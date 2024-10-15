use crate::builder::ActorBuilder;
use crate::dispatcher::Dispatcher;
use crate::handler::{
    AsyncFunc, AsyncHandler, AsyncMessageHandler, AsyncMutatingFunc, AsyncMutatingHandler,
    SyncFunc, SyncHandler, SyncMutatingFunc, SyncMutatingHandler,
};
use crate::message::{ActorMessage, MessageKey, Response, ResponseDowncast};
use crate::types::ActorError;
use ::futures::channel::mpsc;
use smol::future::{self, FutureExt};
use smol::stream::StreamExt;
use smol::LocalExecutor;
use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;
use std::panic::{self, AssertUnwindSafe};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tracing::instrument::WithSubscriber;
use tracing::{debug, error, info, info_span, span, Instrument, Level};

type StateRef<S> = Arc<smol::lock::RwLock<S>>;
type HandlerMap<S, K> = Arc<RwLock<HashMap<MessageKey<K>, Box<dyn AsyncMessageHandler<S>>>>>;

pub struct Actor<S: 'static, K: Eq + Hash + Debug> {
    state: StateRef<S>,
    handlers: HandlerMap<S, K>,
    receiver: mpsc::UnboundedReceiver<ActorMessage<K>>,
}

impl<S: 'static, K: Eq + Hash + Debug + Send + Sync + Clone + 'static> Actor<S, K> {
    pub fn new(state: S) -> (Self, ActorRef<S, K>) {
        let handlers = Arc::new(RwLock::new(HashMap::new()));
        let (sender, receiver) = mpsc::unbounded();
        (
            Self {
                state: Arc::new(smol::lock::RwLock::new(state)),
                handlers: handlers.clone(),
                receiver,
            },
            ActorRef {
                sender,
                handlers: handlers.clone(),
            },
        )
    }

    pub(crate) fn spawn_builder(builder: ActorBuilder<S, K>) -> Result<ActorRef<S, K>, ActorError> {
        let init_state = builder
            .init_state
            .ok_or_else(|| ActorError::SpawnError("init_state function must be provided".into()))?;
        let init_subscriber = builder.init_subscriber.unwrap_or_else(|| Box::new(|| None));
        let name = builder.name.unwrap_or("actor".into());

        info!("Spawning actor...");

        // Create a channel to communicate between threads
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let actor_parent_span = tracing::info_span!("actor root");
        let actor_thread_span = span!(parent: &actor_parent_span, Level::INFO, "actor");
        let actor_parent_span = actor_parent_span.entered();

        // Spawn a new thread for the actor
        info!("Spawning new thread for actor");
        std::thread::Builder::new()
            .name(name.clone())
            .spawn(move || {
                {
                    let tracing_hook = init_subscriber();
                    info!("Building local executor on spawned thread");
                    let exec = LocalExecutor::new();
                    let actor_task_span = info_span!("actor task");
                    let task = async move {
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
                    }
                    .instrument(actor_task_span);
                    future::block_on(exec.run(task).with_current_subscriber());
                    drop(tracing_hook);
                }
                .instrument(actor_thread_span)
            })
            .expect("Failed to spawn actor thread");

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
                            if let Err(e) = handler.handle(self.state.clone(), payload).await {
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
                            let result = handler.handle(self.state.clone(), payload).await;
                            let _ = response_tx.send(result);
                        } else {
                            error!("No handler found for key: {:?}", key);
                            let _ = response_tx.send(Err(ActorError::HandlerNotFound));
                        }
                    }
                }
                // smol::future::yield_now().await;
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

pub struct ActorRef<S, K: Eq + Hash + Debug> {
    sender: mpsc::UnboundedSender<ActorMessage<K>>,
    handlers: HandlerMap<S, K>,
}

impl<S, K: Eq + Hash + Debug> Clone for ActorRef<S, K> {
    fn clone(&self) -> Self {
        ActorRef {
            sender: self.sender.clone(),
            handlers: self.handlers.clone(),
        }
    }
}

impl<S: 'static, K: Eq + Hash + Debug + Clone> ActorRef<S, K> {
    pub async fn ask<M: Send + 'static, R: Send + Debug + 'static, D: Dispatcher<K>>(
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

    pub fn tell<M: Send + 'static, D: Dispatcher<K>>(
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

    pub fn register_handler_async<C, R, E, F>(&self, key: MessageKey<K>, handler: F)
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

    pub fn register_handler_sync<C, R, E, F>(&self, key: MessageKey<K>, handler: F)
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

    pub fn register_handler_async_mutating<C, R, E, F>(&self, key: MessageKey<K>, handler: F)
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

    pub fn register_handler_sync_mutating<C, R, E, F>(&self, key: MessageKey<K>, handler: F)
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

pub struct HandlerRegistration<'a, S: 'static, D: Dispatcher<K>, K: Eq + Hash + Debug> {
    pub actor_ref: &'a ActorRef<S, K>,
    pub dispatcher: &'a mut D,
}

// Tests
#[cfg(test)]
mod tests {
    use crate::message::{Message, MessageDowncast, Response};

    use super::*;
    use std::thread::sleep;

    #[derive(Clone, Debug, PartialEq, Eq, Hash)]
    enum TestMessageKey {
        Increment,
        Append,
        GetCounter,
        GetMessages,
    }

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

    impl Dispatcher<TestMessageKey> for TestDispatcher {
        fn message_key(
            &self,
            message: &dyn Message,
        ) -> Result<MessageKey<TestMessageKey>, ActorError> {
            if message.is::<IncrementMessage>() {
                Ok(MessageKey(TestMessageKey::Increment))
            } else if message.is::<AppendMessage>() {
                Ok(MessageKey(TestMessageKey::Append))
            } else if message.is::<GetCounterMessage>() {
                Ok(MessageKey(TestMessageKey::GetCounter))
            } else if message.is::<GetMessagesMessage>() {
                Ok(MessageKey(TestMessageKey::GetMessages))
            } else {
                Err(ActorError::DispatchError)
            }
        }

        fn wrap(
            &self,
            message: Box<dyn Message>,
            _key: MessageKey<TestMessageKey>,
        ) -> Result<Box<dyn Message>, ActorError> {
            debug!("Wrapping message of type: {:?}", message.type_id());
            Ok(message)
        }

        fn unwrap(
            &self,
            message: Box<dyn Response>,
            _key: MessageKey<TestMessageKey>,
        ) -> Result<Box<dyn Response>, ActorError> {
            debug!("Unwrapping message of type: {:?}", message.type_id());
            Ok(message)
        }
    }

    impl ActorRef<TestState, TestMessageKey> {
        fn register_handlers(&mut self) {
            self.register_handler_async_mutating(
                MessageKey(TestMessageKey::Increment),
                increment_handler,
            );

            self.register_handler_async_mutating(
                MessageKey(TestMessageKey::Append),
                append_handler,
            );

            self.register_handler_async_mutating(
                MessageKey(TestMessageKey::GetCounter),
                get_counter_handler,
            );
            self.register_handler_async_mutating(
                MessageKey(TestMessageKey::GetMessages),
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
        let dispatcher = TestDispatcher;
        let mut actor_ref: ActorRef<TestState, TestMessageKey> = ActorBuilder::new()
            .with_state_init(|| {
                Ok(TestState {
                    counter: 0,
                    messages: Vec::new(),
                })
            })
            .spawn()
            .expect("Failed to spawn actor");

        actor_ref.register_handlers();

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
