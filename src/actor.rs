use crate::dispatcher::Dispatcher;
use crate::handler::{
    AsyncFunc, AsyncHandler, AsyncMessageHandler, AsyncMutatingFunc, AsyncMutatingHandler,
    SyncFunc, SyncHandler, SyncMessageHandler, SyncMutatingFunc, SyncMutatingHandler,
};
use crate::message::{
    ActorMessage, Message, MessageDowncast, MessageKey, Response, ResponseDowncast,
};
use crate::types::ActorError;
use ::futures::channel::mpsc;
// use crate::registration::HandlerRegistration;
// use futures::channel::mpsc;
use smol::future::{self, FutureExt};
use smol::lock::futures;
use smol::stream::StreamExt;
use smol::LocalExecutor;
use std::cell::RefCell;
use std::collections::HashMap;
use std::error::Error;
use std::fmt::Debug;
use std::panic::{self, AssertUnwindSafe};
use std::rc::Rc;
use std::time::Duration;
use tracing::{error, info};

type StateRef<S> = Rc<RefCell<S>>;

pub struct Actor<S: 'static> {
    state: StateRef<S>,
    handlers: HashMap<MessageKey, Box<dyn AsyncMessageHandler<S>>>,
    receiver: mpsc::UnboundedReceiver<ActorMessage>,
}

impl<S: 'static> Actor<S> {
    pub fn new(state: S) -> (Self, ActorRef) {
        let (sender, receiver) = mpsc::unbounded();
        (
            Self {
                state: Rc::new(RefCell::new(state)),
                handlers: HashMap::new(),
                receiver,
            },
            ActorRef { sender },
        )
    }

    pub fn register_handler_async<C, R, E, F>(&mut self, key: MessageKey, handler: F)
    where
        C: 'static + Send,
        R: 'static + Send,
        E: std::error::Error + Send + Sync + 'static,
        F: for<'a> AsyncFunc<'a, S, C, R, E> + 'static,
    {
        let handler = AsyncHandler::new(handler);
        self.handlers.insert(key, Box::new(handler));
    }

    pub fn register_handler_sync<C, R, E, F>(&mut self, key: MessageKey, handler: F)
    where
        C: 'static + Send,
        R: 'static + Send,
        E: std::error::Error + Send + Sync + 'static,
        F: for<'a> SyncFunc<'a, S, C, R, E> + 'static,
    {
        let handler = SyncHandler::new(handler);
        self.handlers.insert(key, Box::new(handler));
    }

    pub fn register_handler_async_mutating<C, R, E, F>(&mut self, key: MessageKey, handler: F)
    where
        C: 'static + Send,
        R: 'static + Send,
        E: std::error::Error + Send + Sync + 'static,
        F: for<'a> AsyncMutatingFunc<'a, S, C, R, E> + 'static,
    {
        let handler = AsyncMutatingHandler::new(handler);
        self.handlers.insert(key, Box::new(handler));
    }

    pub fn register_handler_sync_mutating<C, R, E, F>(&mut self, key: MessageKey, handler: F)
    where
        C: 'static + Send,
        R: 'static + Send,
        E: std::error::Error + Send + Sync + 'static,
        F: for<'a> SyncMutatingFunc<'a, S, C, R, E> + 'static,
    {
        let handler = SyncMutatingHandler::new(handler);
        self.handlers.insert(key, Box::new(handler));
    }

    pub fn spawn_local<F, D>(init: F) -> Result<(ActorRef, D), ActorError>
    where
        F: FnOnce() -> Result<(Actor<S>, ActorRef, D), ActorError>,
        F: 'static + Send,
        D: Dispatcher + Send + 'static,
    {
        info!("Initializing spawn_local");

        // Create a channel to communicate between threads
        let (tx, rx) = std::sync::mpsc::sync_channel(1);

        // Spawn a new thread for the actor
        info!("Spawning new thread for actor");
        std::thread::spawn(move || {
            info!("Building local executor on spawned thread");
            let exec = LocalExecutor::new();
            exec.spawn(async move {
                info!("Spawning actor");
                match init() {
                    Ok((mut actor, actor_ref, dispatcher)) => {
                        tx.send(Ok((actor_ref.clone(), dispatcher))).unwrap();
                        info!("Starting actor run loop");
                        let run_result = AssertUnwindSafe(actor.run()).catch_unwind().await;
                        if let Err(panic_info) = run_result {
                            error!("Actor run loop panicked: {:?}", panic_info);
                        }
                    }
                    Err(e) => {
                        error!("Uh oh... Spawn error: {e:?}");
                        tx.send(Err(e)).unwrap();
                    }
                };
                info!("Actor run loop finished");
            })
            .detach();

            future::block_on(exec.run(future::pending::<()>()));
        });

        // Receive the ActorRef and Dispatcher from the spawned thread
        rx.recv_timeout(Duration::from_secs(2))
            .map_err(|e| ActorError::SpawnError(Box::new(e)))
            .and_then(|result| match result {
                Ok((actor_ref, dispatcher)) => {
                    info!("Received ActorRef and Dispatcher from spawned thread");
                    Ok((actor_ref, dispatcher))
                }
                Err(e) => {
                    error!("Failed to receive ActorRef and Dispatcher: {:?}", e);
                    Err(ActorError::SpawnError(Box::<ActorError>::new(e)))
                }
            })
    }

    pub async fn run(&mut self) -> Result<(), ActorError> {
        let result = panic::AssertUnwindSafe(async {
            info!("Actor run loop started");
            while let Some(message) = self.receiver.next().await {
                info!("Actor got a new message...");
                match message {
                    ActorMessage::Notification(key, payload) => {
                        if let Some(handler) = self.handlers.get(&key) {
                            info!("Handling notification with key: {key:?}");
                            let mut state = self.state.borrow_mut();
                            if let Err(e) = handler.handle(&mut *state, payload).await {
                                info!("Handler error: {e:?}");
                            }
                        } else {
                            error!("No handler found for key: {key:?}");
                        }
                    }
                    ActorMessage::Request(key, payload, response_tx) => {
                        if let Some(handler) = self.handlers.get(&key) {
                            info!("Handling request with key: {key:?}");
                            let mut state = self.state.borrow_mut();
                            let result = handler.handle(&mut *state, payload).await;
                            let _ = response_tx.send(result);
                        } else {
                            error!("No handler found for key: {:?}", key);
                            let _ = response_tx.send(Err(ActorError::HandlerNotFound));
                        }
                    }
                }
            }
            info!("Actor run loop finished");
        })
        .catch_unwind()
        .await;

        match result {
            Ok(_) => {
                info!("Actor run loop somehow finished: {:?}", result);
                Ok(())
            }
            Err(panic_info) => {
                error!("Actor run loop panicked: {:?}", panic_info);
                Err(ActorError::HandlerPanicked)
            }
        }
    }
}

#[derive(Clone)]
pub struct ActorRef {
    sender: mpsc::UnboundedSender<ActorMessage>,
}

impl ActorRef {
    pub async fn ask<M: Send + 'static, R: Send + Debug + 'static, D: Dispatcher>(
        &self,
        dispatcher: &D,
        message: M,
    ) -> Result<R, ActorError> {
        let key = dispatcher.message_key(&message)?;
        let wrapped = dispatcher.wrap(Box::new(message), key.clone())?;

        let (response_tx, response_rx) = ::futures::channel::oneshot::channel();

        self.sender
            .unbounded_send(ActorMessage::Request(key.clone(), wrapped, response_tx))
            .map_err(|_| ActorError::SendError)?;

        let result = response_rx.await.map_err(|_| ActorError::SendError)??;
        // println!("Result type: {:?}", std::any::TypeId::of::<R>());
        // let downcast = result.downcast_ref::<R>().unwrap();
        // println!("Actual type: {:?}", downcast);
        println!(
            "in our ask handler we unwrapped the result as: {:?}",
            &result.any_response()
        );
        let unwrapped = dispatcher.unwrap(result, key.clone());
        println!("Unwrapped type: {:?}", unwrapped.any_response().type_id());
        println!("And unwrapped it to {:?}", &unwrapped.any_response());
        unwrapped.and_then(|r| {
            r.downcast::<R>()
                .map(|r| *r)
                .map_err(|_| ActorError::DowncastError)
        })
    }

    pub fn tell<M: Send + 'static, D: Dispatcher>(
        &self,
        dispatcher: &D,
        message: M,
    ) -> Result<(), ActorError> {
        let key = dispatcher.message_key(&message)?;
        let wrapped = dispatcher.wrap(Box::new(message), key.clone())?;
        self.sender
            .unbounded_send(ActorMessage::Notification(key, wrapped))
            .map_err(|_| ActorError::SendError)
    }
}

pub struct HandlerRegistration<'a, S: 'static, D: Dispatcher> {
    pub actor: &'a mut Actor<S>,
    pub dispatcher: &'a mut D,
}

impl<'a, S: 'static, D: Dispatcher> HandlerRegistration<'a, S, D> {
    pub fn register_handler_async<C, R, E, F>(&mut self, key: MessageKey, handler: F)
    where
        C: 'static + Send + Clone,
        R: 'static + Send,
        E: std::error::Error + Send + Sync + 'static,
        F: for<'b> AsyncFunc<'b, S, C, R, E> + 'static,
    {
        // self.dispatcher.register_wrapper(
        //     key.clone(),
        //     Box::new(move |msg| {
        //         wrapper(
        //             msg.downcast_ref::<C>()
        //                 .expect("Type mismatch in wrapper")
        //                 .clone(),
        //         )
        //     }),
        // );
        self.actor.register_handler_async(key, handler);
    }

    pub fn register_handler_sync<C, R, E, F>(&mut self, key: MessageKey, handler: F)
    where
        C: 'static + Send + Clone,
        R: 'static + Send,
        E: std::error::Error + Send + Sync + 'static,
        F: for<'b> SyncFunc<'b, S, C, R, E> + 'static,
    {
        // self.dispatcher.register_wrapper(
        //     key.clone(),
        //     Box::new(move |msg| {
        //         wrapper(
        //             msg.downcast_ref::<C>()
        //                 .expect("Type mismatch in wrapper")
        //                 .clone(),
        //         )
        //     }),
        // );
        self.actor.register_handler_sync(key, handler);
    }

    pub fn register_handler_async_mutating<C, R, E, F>(&mut self, key: MessageKey, handler: F)
    where
        C: 'static + Send + Clone,
        R: 'static + Send,
        E: std::error::Error + Send + Sync + 'static,
        F: for<'b> AsyncMutatingFunc<'b, S, C, R, E> + 'static,
    {
        // self.dispatcher.register_wrapper(
        //     key.clone(),
        //     Box::new(move |msg| {
        //         wrapper(
        //             msg.downcast_ref::<C>()
        //                 .expect("Type mismatch in wrapper")
        //                 .clone(),
        //         )
        //     }),
        // );
        self.actor.register_handler_async_mutating(key, handler);
    }

    pub fn register_handler_sync_mutating<C, R, E, F>(&mut self, key: MessageKey, handler: F)
    where
        C: 'static + Send + Clone,
        R: 'static + Send,
        E: std::error::Error + Send + Sync + 'static,
        F: for<'b> SyncMutatingFunc<'b, S, C, R, E> + 'static,
    {
        // self.dispatcher.register_wrapper(
        //     key.clone(),
        //     Box::new(move |msg| {
        //         wrapper(
        //             msg.downcast_ref::<C>()
        //                 .expect("Type mismatch in wrapper")
        //                 .clone(),
        //         )
        //     }),
        // );
        self.actor.register_handler_sync_mutating(key, handler);
    }
}

// Tests
#[cfg(test)]
mod tests {
    use crate::message::Response;

    use super::*;
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        thread::sleep,
    };

    #[derive(Clone)]
    struct TestState {
        counter: i32,
        messages: Vec<String>,
    }

    #[derive(Clone)]
    struct IncrementMessage(i32);

    #[derive(Clone)]
    struct AppendMessage(String);

    #[derive(Clone)]
    struct GetCounterMessage;

    #[derive(Clone)]
    struct GetMessagesMessage;

    #[derive(Clone)]
    struct TestDispatcher;

    impl Dispatcher for TestDispatcher {
        fn message_key(&self, message: &dyn Message) -> Result<MessageKey, ActorError> {
            if message.is::<IncrementMessage>() {
                Ok(MessageKey::new("increment"))
            } else if message.is::<AppendMessage>() {
                Ok(MessageKey::new("append"))
            } else if message.is::<GetCounterMessage>() {
                Ok(MessageKey::new("get_counter"))
            } else if message.is::<GetMessagesMessage>() {
                Ok(MessageKey::new("get_messages"))
            } else {
                Err(ActorError::DispatchError)
            }
        }

        fn wrap(
            &self,
            message: Box<dyn Message>,
            _key: MessageKey,
        ) -> Result<Box<dyn Message>, ActorError> {
            println!("Wrapping message of type: {:?}", message.type_id());
            Ok(message)
        }

        fn unwrap(
            &self,
            message: Box<dyn Response>,
            _key: MessageKey,
        ) -> Result<Box<dyn Response>, ActorError> {
            println!("Unwrapping message of type: {:?}", message.type_id());
            Ok(message)
        }

        // fn register_wrapper(
        //     &mut self,
        //     _key: MessageKey,
        //     _wrapper: Box<dyn Fn(Box<dyn Message>) -> Box<dyn Message> + Send + Sync>,
        // ) {
        //     // In this simple implementation, we don't need to do anything
        // }
    }

    async fn increment_handler(
        state: &mut TestState,
        msg: IncrementMessage,
    ) -> Result<(), ActorError> {
        state.counter += msg.0;
        Ok(())
    }

    async fn append_handler(state: &mut TestState, msg: AppendMessage) -> Result<(), ActorError> {
        state.messages.push(msg.0);
        Ok(())
    }

    async fn get_counter_handler(
        state: &mut TestState,
        _: GetCounterMessage,
    ) -> Result<i32, ActorError> {
        Ok(state.counter)
    }

    async fn get_messages_handler(
        state: &mut TestState,
        _: GetMessagesMessage,
    ) -> Result<Vec<String>, ActorError> {
        Ok(state.messages.clone())
    }

    async fn test_actor_with_custom_dispatcher() {
        let (actor_ref, dispatcher) = Actor::spawn_local(|| {
            let mut dispatcher = TestDispatcher;
            let (mut actor, actor_ref) = Actor::new(TestState {
                counter: 0,
                messages: Vec::new(),
            });

            let mut registration = HandlerRegistration {
                actor: &mut actor,
                dispatcher: &mut dispatcher,
            };

            registration.register_handler_async_mutating(
                MessageKey::new("increment"),
                // |msg: IncrementMessage| Box::new(msg) as Box<dyn Message>,
                increment_handler,
            );

            registration.register_handler_async_mutating(
                MessageKey::new("append"),
                // |msg: AppendMessage| Box::new(msg) as Box<dyn Message>,
                append_handler,
            );

            registration.register_handler_async_mutating(
                MessageKey::new("get_counter"),
                // |msg: GetCounterMessage| Box::new(msg) as Box<dyn Message>,
                get_counter_handler,
            );

            registration.register_handler_async_mutating(
                MessageKey::new("get_messages"),
                // |msg: GetMessagesMessage| Box::new(msg) as Box<dyn Message>,
                get_messages_handler,
            );

            Ok((actor, actor_ref, dispatcher))
        })
        .expect("Failed to spawn actor");

        // Test increment
        actor_ref
            .tell(&dispatcher, IncrementMessage(5))
            .expect("Failed to send increment 5");
        actor_ref
            .tell(&dispatcher, IncrementMessage(3))
            .expect("Failed to send increment 3");

        // Test append
        actor_ref
            .tell(&dispatcher, AppendMessage("Hello".to_string()))
            .expect("Failed to send append Hello");
        actor_ref
            .tell(&dispatcher, AppendMessage("World".to_string()))
            .expect("Failed to send append World");

        // Allow some time for processing
        sleep(Duration::from_millis(100));

        // Test get_counter
        let counter: i32 = actor_ref
            .ask(&dispatcher, GetCounterMessage)
            .await
            .unwrap_or_else(|e| {
                println!("Error getting counter: {:?}", e);
                panic!("Failed to get counter: {:?}", e);
            });
        assert_eq!(counter, 8);

        // Test get_messages
        let messages: Vec<String> = actor_ref
            .ask(&dispatcher, GetMessagesMessage)
            .await
            .expect("Failed to get messages");
        assert_eq!(messages, vec!["Hello".to_string(), "World".to_string()]);

        println!("All tests passed successfully!");
    }
}
