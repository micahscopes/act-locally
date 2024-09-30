use crate::dispatcher::Dispatcher;
use crate::handler::{
    AsyncFunc, AsyncHandler, AsyncMessageHandler, AsyncMutatingFunc, AsyncMutatingHandler,
    SyncFunc, SyncHandler, SyncMutatingFunc, SyncMutatingHandler,
};
use crate::message::{ActorMessage, MessageKey, Response, ResponseDowncast};
use crate::types::ActorError;
use ::futures::channel::mpsc;
// use crate::registration::HandlerRegistration;
// use futures::channel::mpsc;
use smol::future::{self, FutureExt};
use smol::stream::StreamExt;
use smol::LocalExecutor;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::Debug;
use std::panic::{self, AssertUnwindSafe};
use std::rc::Rc;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tracing::instrument::WithSubscriber;
use tracing::subscriber::{set_default, DefaultGuard};
use tracing::{debug, error, info, info_span, span, Instrument, Level, Span, Subscriber};

type StateRef<S> = Rc<RefCell<S>>;
type HandlerMap<S> = Arc<RwLock<HashMap<MessageKey, Box<dyn AsyncMessageHandler<S>>>>>;

pub struct Actor<S: 'static> {
    state: StateRef<S>,
    handlers: HandlerMap<S>,
    receiver: mpsc::UnboundedReceiver<ActorMessage>,
}

impl<S: 'static> Actor<S> {
    pub fn new(state: S) -> (Self, ActorRef<S>) {
        let handlers = Arc::new(RwLock::new(HashMap::new()));
        let (sender, receiver) = mpsc::unbounded();
        (
            Self {
                state: Rc::new(RefCell::new(state)),
                handlers: handlers.clone(),
                receiver,
            },
            ActorRef {
                sender,
                handlers: handlers.clone(),
            },
        )
    }
    pub fn spawn<F>(init_state: F) -> Result<ActorRef<S>, ActorError>
    where
        F: FnOnce() -> Result<S, ActorError>,
        F: 'static + Send,
    {
        Self::setup_spawn_with_subscriber(init_state, || None)
    }

    pub fn spawn_with_tracing<F, T>(
        init_state: F,
        init_subscriber: T,
    ) -> Result<ActorRef<S>, ActorError>
    where
        F: FnOnce() -> Result<S, ActorError>,
        F: 'static + Send,
        T: FnOnce() -> Option<DefaultGuard>,
        T: 'static + Send,
    {
        Self::setup_spawn_with_subscriber(init_state, init_subscriber)
    }

    fn setup_spawn_with_subscriber<F, T>(
        init_state: F,
        init_subscriber: T,
    ) -> Result<ActorRef<S>, ActorError>
    where
        F: FnOnce() -> Result<S, ActorError>,
        F: 'static + Send,
        T: FnOnce() -> Option<DefaultGuard>,
        T: 'static + Send,
    {
        info!("Spawning actor...");

        // Create a channel to communicate between threads
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let actor_parent_span = tracing::info_span!("actor", y = 22);
        let actor_thread_span = span!(parent: &actor_parent_span, Level::INFO, "actor", x = 32);
        let actor_parent_span = actor_parent_span.entered();
        // .child_of(&actor_parent_span);

        // Spawn a new thread for the actor
        info!("Spawning new thread for actor");
        std::thread::spawn(move || {
            {
                let tracing_hook = init_subscriber();
                info!("Building local executor on spawned thread");
                let exec = LocalExecutor::new();
                let actor_task_span = info_span!("actor task", z = 44);
                let task = exec.spawn(
                    async move {
                        // let actor_task_span = actor_task_span.enter();
                        info!("Constructing actor state");
                        match init_state() {
                            Ok(state) => {
                                let (mut actor, actor_ref) = Actor::new(state);
                                tx.send(Ok(actor_ref.clone())).unwrap();
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
                        // drop(actor_task_span);
                    }
                    .instrument(actor_task_span),
                );

                future::block_on(exec.run(task).with_current_subscriber());
                drop(tracing_hook);
            }
            .instrument(actor_thread_span)
        });

        drop(actor_parent_span);
        // Receive the ActorRef and Dispatcher from the spawned thread
        rx.recv_timeout(Duration::from_secs(2))
            .map_err(|e| ActorError::SpawnError(Box::new(e)))
            .and_then(|result| match result {
                Ok(actor_ref) => {
                    info!("Received ActorRef and Dispatcher from spawned thread");
                    Ok(actor_ref)
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
                        let handlers = self.handlers.read().unwrap();
                        if let Some(handler) = handlers.get(&key) {
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
                        let handlers = self.handlers.read().unwrap();
                        if let Some(handler) = handlers.get(&key) {
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

pub struct ActorRef<S> {
    sender: mpsc::UnboundedSender<ActorMessage>,
    handlers: HandlerMap<S>,
}

impl<S> Clone for ActorRef<S> {
    fn clone(&self) -> Self {
        ActorRef {
            sender: self.sender.clone(),
            handlers: self.handlers.clone(),
        }
    }
}

impl<S: 'static> ActorRef<S> {
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
        debug!(
            "in our ask handler we unwrapped the result as: {:?}",
            &result.any_response()
        );
        let unwrapped = dispatcher.unwrap(result, key.clone());
        debug!("Unwrapped type: {:?}", unwrapped.any_response().type_id());
        debug!("And unwrapped it to {:?}", &unwrapped.any_response());
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

    pub fn register_handler_async<C, R, E, F>(&self, key: MessageKey, handler: F)
    where
        C: 'static + Send,
        R: 'static + Send,
        E: std::error::Error + Send + Sync + 'static,
        F: for<'a> AsyncFunc<'a, S, C, R, E> + 'static,
    {
        let handler = AsyncHandler::new(handler);
        if let Ok(mut handlers) = self.handlers.write() {
            handlers.insert(key, Box::new(handler));
        }
    }

    pub fn register_handler_sync<C, R, E, F>(&self, key: MessageKey, handler: F)
    where
        C: 'static + Send,
        R: 'static + Send,
        E: std::error::Error + Send + Sync + 'static,
        F: for<'a> SyncFunc<'a, S, C, R, E> + 'static,
    {
        let handler = SyncHandler::new(handler);
        if let Ok(mut handlers) = self.handlers.write() {
            handlers.insert(key, Box::new(handler));
        }
    }

    pub fn register_handler_async_mutating<C, R, E, F>(&self, key: MessageKey, handler: F)
    where
        C: 'static + Send,
        R: 'static + Send,
        E: std::error::Error + Send + Sync + 'static,
        F: for<'a> AsyncMutatingFunc<'a, S, C, R, E> + 'static,
    {
        let handler = AsyncMutatingHandler::new(handler);
        if let Ok(mut handlers) = self.handlers.write() {
            handlers.insert(key, Box::new(handler));
        }
    }

    pub fn register_handler_sync_mutating<C, R, E, F>(&self, key: MessageKey, handler: F)
    where
        C: 'static + Send,
        R: 'static + Send,
        E: std::error::Error + Send + Sync + 'static,
        F: for<'a> SyncMutatingFunc<'a, S, C, R, E> + 'static,
    {
        let handler = SyncMutatingHandler::new(handler);
        if let Ok(mut handlers) = self.handlers.write() {
            handlers.insert(key, Box::new(handler));
        }
    }
}

pub struct HandlerRegistration<'a, S: 'static, D: Dispatcher> {
    pub actor_ref: &'a ActorRef<S>,
    pub dispatcher: &'a mut D,
}

// Tests
#[cfg(test)]
mod tests {
    use crate::message::{Message, MessageDowncast, Response};

    use super::*;
    use std::{borrow::BorrowMut, thread::sleep};

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
            debug!("Wrapping message of type: {:?}", message.type_id());
            Ok(message)
        }

        fn unwrap(
            &self,
            message: Box<dyn Response>,
            _key: MessageKey,
        ) -> Result<Box<dyn Response>, ActorError> {
            debug!("Unwrapping message of type: {:?}", message.type_id());
            Ok(message)
        }
    }

    impl<'a> HandlerRegistration<'a, TestState, TestDispatcher> {
        fn register_handlers(&mut self) {
            let actor_ref = &mut self.actor_ref.borrow_mut();
            // let dispatcher = &self.dispatcher;
            actor_ref
                .register_handler_async_mutating(MessageKey::new("increment"), increment_handler);

            actor_ref.register_handler_async_mutating(MessageKey::new("append"), append_handler);

            actor_ref.register_handler_async_mutating(
                MessageKey::new("get_counter"),
                get_counter_handler,
            );
            actor_ref.register_handler_async_mutating(
                MessageKey::new("get_messages"),
                get_messages_handler,
            );
        }
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

    #[test]
    fn test_actor_with_custom_dispatcher() {
        let mut dispatcher = TestDispatcher;
        let actor_ref = Actor::spawn(|| {
            Ok(TestState {
                counter: 0,
                messages: Vec::new(),
            })
        })
        .expect("Failed to spawn actor");

        HandlerRegistration {
            actor_ref: &mut actor_ref.clone(),
            dispatcher: &mut dispatcher,
        }
        .register_handlers();

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
        future::block_on(async move {
            let counter: i32 = actor_ref
                .ask(&dispatcher, GetCounterMessage)
                .await
                .unwrap_or_else(|e| {
                    println!("Error getting counter: {:?}", e);
                    panic!("Failed to get counter: {:?}", e);
                });
            assert_eq!(counter, 8);

            let messages: Vec<String> = actor_ref
                .ask(&dispatcher, GetMessagesMessage)
                .await
                .expect("Failed to get messages");
            assert_eq!(messages, vec!["Hello".to_string(), "World".to_string()]);
        });

        // future::block_on(task);

        println!("All tests passed successfully!");
    }
}
