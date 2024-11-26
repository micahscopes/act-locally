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
use smol::lock::RwLock;
use smol::stream::StreamExt;
use smol::LocalExecutor;
use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;
use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;
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
                    let task = async {
                        // let actor_task_span = actor_task_span.enter();
                        info!("Constructing actor state");
                        match init_state() {
                            Ok(state) => {
                                let (mut actor, actor_ref) = Actor::new(state);
                                tx.send(Ok(actor_ref.clone())).unwrap();
                                info!("Starting actor run loop");
                                let run_result =
                                    AssertUnwindSafe(actor.run(&exec)).catch_unwind().await;
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
                    drop(tracing_hook)
                };
                ().instrument(actor_thread_span)
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

    pub async fn run(&mut self, exec: &LocalExecutor<'static>) -> Result<(), ActorError> {
        let result = panic::AssertUnwindSafe(async {
            info!("Actor run loop started");
            while let Some(message) = self.receiver.next().await {
                info!("Actor got a new message...");
                let handlers = self.handlers.clone();
                let state = self.state.clone();
                let task = async move {
                    match message {
                        ActorMessage::Notification(key, payload) => {
                            let handlers = handlers.read().await;
                            if let Some(handler) = handlers.get(&key) {
                                info!("Handling notification with key: {key:?}");
                                if let Err(e) = handler.handle(state, payload).await {
                                    info!("Handler error: {e:?}");
                                }
                            } else {
                                error!("No handler found for key: {key:?}");
                            }
                        }
                        ActorMessage::Request(key, payload, response_tx) => {
                            let handlers = handlers.read().await;
                            if let Some(handler) = handlers.get(&key) {
                                info!("Handling request with key: {key:?}");
                                let result = handler.handle(state, payload).await;
                                let _ = response_tx.send(result);
                            } else {
                                error!("No handler found for key: {:?}", key);
                                let _ = response_tx.send(Err(ActorError::HandlerNotFound));
                            }
                        }
                    }
                };
                exec.spawn(task).detach();
                // exec.tick();
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
        let mut handlers = self.handlers.write_blocking();
        handlers.insert(key, Box::new(handler));
    }

    pub fn register_handler_sync<C, R, E, F>(&self, key: MessageKey<K>, handler: F)
    where
        C: 'static + Send,
        R: 'static + Send,
        E: std::error::Error + Send + Sync + 'static,
        F: for<'a> SyncFunc<'a, S, C, R, E> + 'static,
    {
        let handler = SyncHandler::new(handler);
        let mut handlers = self.handlers.write_blocking();
        handlers.insert(key, Box::new(handler));
    }

    pub fn register_handler_async_mutating<C, R, E, F>(&self, key: MessageKey<K>, handler: F)
    where
        C: 'static + Send,
        R: 'static + Send,
        E: std::error::Error + Send + Sync + 'static,
        F: for<'a> AsyncMutatingFunc<'a, S, C, R, E> + 'static,
    {
        let handler = AsyncMutatingHandler::new(handler);
        let mut handlers = self.handlers.write_blocking();
        handlers.insert(key, Box::new(handler));
    }

    pub fn register_handler_sync_mutating<C, R, E, F>(&self, key: MessageKey<K>, handler: F)
    where
        C: 'static + Send,
        R: 'static + Send,
        E: std::error::Error + Send + Sync + 'static,
        F: for<'a> SyncMutatingFunc<'a, S, C, R, E> + 'static,
    {
        let handler = SyncMutatingHandler::new(handler);
        let mut handlers = self.handlers.write_blocking();
        handlers.insert(key, Box::new(handler));
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
    use std::{
        sync::{
            atomic::{AtomicUsize, Ordering},
            OnceLock,
        },
        thread::sleep,
    };

    #[derive(Clone, Debug, PartialEq, Eq, Hash)]
    enum TestMessageKey {
        Increment,
        Append,
        GetCounter,
        GetMessages,
    }

    #[derive(Clone, Debug)]
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

        println!("All tests passed successfully!");
    }

    type StaticExecutionLog = OnceLock<Arc<smol::lock::Mutex<Vec<(usize, String, &'static str)>>>>;

    #[test]
    fn test_yielding_handlers() {
        // TODO: also cover sync handlers
        use std::sync::Arc;
        static EXECUTION_LOG: StaticExecutionLog = OnceLock::new();
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
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

        async fn yielding_nonmutating_handler(
            _state: &TestState,
            AppendMessage(msg): AppendMessage,
        ) -> Result<(), ActorError> {
            let id = COUNTER.fetch_add(1, Ordering::SeqCst);
            {
                let mut log = EXECUTION_LOG
                    .get_or_init(|| Arc::new(smol::lock::Mutex::new(Vec::new())))
                    .lock()
                    .await;
                log.push((id, msg.clone(), "start"));
            }

            smol::future::yield_now().await;

            {
                let mut log = EXECUTION_LOG
                    .get_or_init(|| Arc::new(smol::lock::Mutex::new(Vec::new())))
                    .lock()
                    .await;
                log.push((id, msg.clone(), "end"));
            }
            Ok(())
        }

        actor_ref.register_handler_async(
            MessageKey(TestMessageKey::Append),
            yielding_nonmutating_handler,
        );

        async fn yielding_mutating_handler(
            state: &mut TestState,
            IncrementMessage(msg): IncrementMessage,
        ) -> Result<(), ActorError> {
            let id = COUNTER.fetch_add(1, Ordering::SeqCst);
            {
                let mut log = EXECUTION_LOG
                    .get_or_init(|| Arc::new(smol::lock::Mutex::new(Vec::new())))
                    .lock()
                    .await;
                log.push((id, format!("+{}", msg.clone()), "start"));
            }

            smol::future::yield_now().await;

            {
                let mut log = EXECUTION_LOG
                    .get_or_init(|| Arc::new(smol::lock::Mutex::new(Vec::new())))
                    .lock()
                    .await;
                log.push((id, format!("+{}", msg.clone()), "end"));
            }

            state.counter += msg;
            Ok(())
        }

        actor_ref.register_handler_async_mutating(
            MessageKey(TestMessageKey::Increment),
            yielding_mutating_handler,
        );

        future::block_on(async move {
            actor_ref
                .tell(&dispatcher, AppendMessage("A".to_string()))
                .unwrap();
            actor_ref
                .tell(&dispatcher, AppendMessage("B".to_string()))
                .unwrap();
            actor_ref
                .tell(&dispatcher, AppendMessage("C".to_string()))
                .unwrap();

            actor_ref
                .tell(&dispatcher, IncrementMessage(1))
                .expect("Failed to send increment 1");
            actor_ref
                .tell(&dispatcher, IncrementMessage(2))
                .expect("Failed to send increment 2");
            actor_ref
                .tell(&dispatcher, IncrementMessage(3))
                .expect("Failed to send increment 3");

            // Allow some time for processing
            smol::Timer::after(Duration::from_millis(1)).await;

            let final_order = EXECUTION_LOG
                .get()
                .expect("EXECUTION_ORDER should be initialized")
                .lock()
                .await
                .clone();

            insta::assert_snapshot!(final_order
                .iter()
                .map(|(id, msg, state)| format!("({id}, {msg}, {state})"))
                .collect::<Vec<String>>()
                .join("\n"));
        });
    }

    // COMPREHENSIVE CONCURRENCY TESTING
    #[derive(Clone, Debug, Default)]
    struct AsyncNonMutMessage(Option<usize>);
    #[derive(Clone, Debug, Default)]
    struct AsyncMutMessage(Option<usize>);
    #[derive(Clone, Debug, Default)]
    struct SyncNonMutMessage(Option<usize>);
    #[derive(Clone, Debug, Default)]
    struct SyncMutMessage(Option<usize>);
    #[derive(Clone, Debug)]
    struct GetStateMessage;

    #[derive(Clone, Debug, PartialEq, Eq, Hash)]
    enum ConcurrentTestMessageKey {
        AsyncNonMut,
        AsyncMut,
        SyncNonMut,
        SyncMut,
        GetState,
    }

    struct ConcurrentTestDispatcher {
        counter: AtomicUsize,
    }

    impl ConcurrentTestDispatcher {
        fn new() -> Self {
            ConcurrentTestDispatcher {
                counter: AtomicUsize::new(0),
            }
        }
    }

    impl Dispatcher<ConcurrentTestMessageKey> for ConcurrentTestDispatcher {
        fn message_key(
            &self,
            message: &dyn Message,
        ) -> Result<MessageKey<ConcurrentTestMessageKey>, ActorError> {
            if message.is::<AsyncNonMutMessage>() {
                Ok(MessageKey(ConcurrentTestMessageKey::AsyncNonMut))
            } else if message.is::<AsyncMutMessage>() {
                Ok(MessageKey(ConcurrentTestMessageKey::AsyncMut))
            } else if message.is::<SyncNonMutMessage>() {
                Ok(MessageKey(ConcurrentTestMessageKey::SyncNonMut))
            } else if message.is::<SyncMutMessage>() {
                Ok(MessageKey(ConcurrentTestMessageKey::SyncMut))
            } else if message.is::<GetStateMessage>() {
                Ok(MessageKey(ConcurrentTestMessageKey::GetState))
            } else {
                Err(ActorError::DispatchError)
            }
        }

        fn wrap(
            &self,
            message: Box<dyn Message>,
            key: MessageKey<ConcurrentTestMessageKey>,
        ) -> Result<Box<dyn Message>, ActorError> {
            let order = self.counter.fetch_add(1, Ordering::SeqCst);
            match key.0 {
                ConcurrentTestMessageKey::AsyncNonMut => {
                    let mut msg = message.downcast::<AsyncNonMutMessage>().unwrap();
                    *msg = AsyncNonMutMessage(Some(order));
                    Ok(msg)
                }
                ConcurrentTestMessageKey::AsyncMut => {
                    let mut msg = message.downcast::<AsyncMutMessage>().unwrap();
                    *msg = AsyncMutMessage(Some(order));
                    Ok(msg)
                }
                ConcurrentTestMessageKey::SyncNonMut => {
                    let mut msg = message.downcast::<SyncNonMutMessage>().unwrap();
                    *msg = SyncNonMutMessage(Some(order));
                    Ok(msg)
                }
                ConcurrentTestMessageKey::SyncMut => {
                    let mut msg = message.downcast::<SyncMutMessage>().unwrap();
                    *msg = SyncMutMessage(Some(order));
                    Ok(msg)
                }
                ConcurrentTestMessageKey::GetState => Ok(message),
            }
        }

        fn unwrap(
            &self,
            message: Box<dyn Response>,
            _key: MessageKey<ConcurrentTestMessageKey>,
        ) -> Result<Box<dyn Response>, ActorError> {
            Ok(message)
        }
    }

    #[derive(Clone)]
    struct ConcurrencyTestState {
        execution_log: Arc<smol::lock::Mutex<Vec<String>>>,
        id_counter: Arc<AtomicUsize>,
    }

    impl std::fmt::Debug for ConcurrencyTestState {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("ConcurrencyTestState")
                // .field("counter", &self.counter)
                // .field("messages", &self.messages)
                .finish()
        }
    }

    async fn async_non_mut_handler(
        state: &ConcurrencyTestState,
        AsyncNonMutMessage(msg): AsyncNonMutMessage,
    ) -> Result<(), ActorError> {
        let id = state.id_counter.fetch_add(1, Ordering::SeqCst);
        log_execution(
            &state.execution_log,
            id,
            &msg.unwrap().to_string(),
            "AsyncNonMut",
            "start",
        )
        .await;
        smol::future::yield_now().await;
        log_execution(
            &state.execution_log,
            id,
            &msg.unwrap().to_string(),
            "AsyncNonMut",
            "end",
        )
        .await;
        Ok(())
    }

    async fn async_mut_handler(
        state: &mut ConcurrencyTestState,
        AsyncMutMessage(msg): AsyncMutMessage,
    ) -> Result<(), ActorError> {
        let id = state.id_counter.fetch_add(1, Ordering::SeqCst);
        log_execution(
            &state.execution_log,
            id,
            &msg.unwrap().to_string(),
            "AsyncMut",
            "start",
        )
        .await;
        smol::future::yield_now().await;
        log_execution(
            &state.execution_log,
            id,
            &msg.unwrap().to_string(),
            "AsyncMut",
            "end",
        )
        .await;
        Ok(())
    }

    fn sync_non_mut_handler(
        state: &ConcurrencyTestState,
        SyncNonMutMessage(msg): SyncNonMutMessage,
    ) -> Result<(), ActorError> {
        let id = state.id_counter.fetch_add(1, Ordering::SeqCst);
        smol::block_on(log_execution(
            &state.execution_log,
            id,
            &msg.unwrap().to_string(),
            "SyncNonMut",
            "start",
        ));
        std::thread::sleep(Duration::from_millis(3));
        smol::block_on(log_execution(
            &state.execution_log,
            id,
            &msg.unwrap().to_string(),
            "SyncNonMut",
            "end",
        ));
        Ok(())
    }

    fn sync_mut_handler(
        state: &mut ConcurrencyTestState,
        SyncMutMessage(msg): SyncMutMessage,
    ) -> Result<(), ActorError> {
        let id = state.id_counter.fetch_add(1, Ordering::SeqCst);
        smol::block_on(log_execution(
            &state.execution_log,
            id,
            &msg.unwrap().to_string(),
            "SyncMut",
            "start",
        ));
        std::thread::sleep(Duration::from_millis(3));
        smol::block_on(log_execution(
            &state.execution_log,
            id,
            &msg.unwrap().to_string(),
            "SyncMut",
            "end",
        ));
        Ok(())
    }

    fn get_state_handler(
        state: &ConcurrencyTestState,
        _: GetStateMessage,
    ) -> Result<ConcurrencyTestState, ActorError> {
        Ok(state.clone())
    }

    async fn log_execution(
        log: &Arc<smol::lock::Mutex<Vec<String>>>,
        id: usize,
        msg: &str,
        handler_type: &str,
        state: &str,
    ) {
        let mut log = log.lock().await;
        log.push(format!("({id}, {msg}, {handler_type}, {state})"));
    }

    #[test]
    fn test_comprehensive_handler_execution_order() {
        let dispatcher = ConcurrentTestDispatcher::new();
        let actor_ref: ActorRef<ConcurrencyTestState, ConcurrentTestMessageKey> =
            ActorBuilder::new()
                .with_state_init(|| {
                    Ok(ConcurrencyTestState {
                        execution_log: Arc::new(smol::lock::Mutex::new(Vec::new())),
                        id_counter: Arc::new(AtomicUsize::new(0)),
                    })
                })
                .spawn()
                .expect("Failed to spawn actor");

        actor_ref.register_handler_sync(
            MessageKey(ConcurrentTestMessageKey::GetState),
            get_state_handler,
        );

        actor_ref.register_handler_async(
            MessageKey(ConcurrentTestMessageKey::AsyncNonMut),
            async_non_mut_handler,
        );
        actor_ref.register_handler_async_mutating(
            MessageKey(ConcurrentTestMessageKey::AsyncMut),
            async_mut_handler,
        );
        actor_ref.register_handler_sync(
            MessageKey(ConcurrentTestMessageKey::SyncNonMut),
            sync_non_mut_handler,
        );
        actor_ref.register_handler_sync_mutating(
            MessageKey(ConcurrentTestMessageKey::SyncMut),
            sync_mut_handler,
        );

        future::block_on(async move {
            // Interleave async and sync non-mutating messages, punctuated by mutating ones
            actor_ref
                .tell(&dispatcher, AsyncNonMutMessage::default())
                .unwrap();
            actor_ref
                .tell(&dispatcher, SyncNonMutMessage::default())
                .unwrap();
            actor_ref
                .tell(&dispatcher, AsyncNonMutMessage::default())
                .unwrap();
            actor_ref
                .tell(&dispatcher, AsyncMutMessage::default())
                .unwrap();
            actor_ref
                .tell(&dispatcher, SyncNonMutMessage::default())
                .unwrap();
            actor_ref
                .tell(&dispatcher, AsyncNonMutMessage::default())
                .unwrap();
            actor_ref
                .tell(&dispatcher, SyncMutMessage::default())
                .unwrap();
            actor_ref
                .tell(&dispatcher, AsyncNonMutMessage::default())
                .unwrap();
            actor_ref
                .tell(&dispatcher, SyncNonMutMessage::default())
                .unwrap();
            actor_ref
                .tell(&dispatcher, AsyncMutMessage::default())
                .unwrap();
            actor_ref
                .tell(&dispatcher, AsyncNonMutMessage::default())
                .unwrap();
            actor_ref
                .tell(&dispatcher, SyncNonMutMessage::default())
                .unwrap();

            // Allow time for processing
            smol::Timer::after(Duration::from_millis(50)).await;

            let state: ConcurrencyTestState = actor_ref
                .ask(&dispatcher, GetStateMessage)
                .await
                .expect("Failed to get state");

            let final_order = state.execution_log.lock().await.clone();

            insta::assert_snapshot!(final_order.join("\n"));
        });
    }

    #[test]
    fn test_async_handlers_execution_order() {
        let dispatcher = ConcurrentTestDispatcher::new();
        let actor_ref: ActorRef<ConcurrencyTestState, ConcurrentTestMessageKey> =
            ActorBuilder::new()
                .with_state_init(|| {
                    Ok(ConcurrencyTestState {
                        execution_log: Arc::new(smol::lock::Mutex::new(Vec::new())),
                        id_counter: Arc::new(AtomicUsize::new(0)),
                    })
                })
                .spawn()
                .expect("Failed to spawn actor");

        actor_ref.register_handler_async(
            MessageKey(ConcurrentTestMessageKey::AsyncNonMut),
            async_non_mut_handler,
        );
        actor_ref.register_handler_async_mutating(
            MessageKey(ConcurrentTestMessageKey::AsyncMut),
            async_mut_handler,
        );
        actor_ref.register_handler_sync(
            MessageKey(ConcurrentTestMessageKey::GetState),
            get_state_handler,
        );

        future::block_on(async move {
            actor_ref
                .tell(&dispatcher, AsyncNonMutMessage::default())
                .unwrap();
            actor_ref
                .tell(&dispatcher, AsyncNonMutMessage::default())
                .unwrap();
            actor_ref
                .tell(&dispatcher, AsyncMutMessage::default())
                .unwrap();
            actor_ref
                .tell(&dispatcher, AsyncNonMutMessage::default())
                .unwrap();
            actor_ref
                .tell(&dispatcher, AsyncNonMutMessage::default())
                .unwrap();
            actor_ref
                .tell(&dispatcher, AsyncMutMessage::default())
                .unwrap();
            actor_ref
                .tell(&dispatcher, AsyncNonMutMessage::default())
                .unwrap();

            // Allow time for processing
            smol::Timer::after(Duration::from_millis(50)).await;

            let state: ConcurrencyTestState = actor_ref
                .ask(&dispatcher, GetStateMessage)
                .await
                .expect("Failed to get state");

            let final_order = state.execution_log.lock().await.clone();

            insta::assert_snapshot!(final_order.join("\n"));
        });
    }

    #[test]
    fn test_sync_handlers_execution_order() {
        let dispatcher = ConcurrentTestDispatcher::new();
        let actor_ref: ActorRef<ConcurrencyTestState, ConcurrentTestMessageKey> =
            ActorBuilder::new()
                .with_state_init(|| {
                    Ok(ConcurrencyTestState {
                        execution_log: Arc::new(smol::lock::Mutex::new(Vec::new())),
                        id_counter: Arc::new(AtomicUsize::new(0)),
                    })
                })
                .spawn()
                .expect("Failed to spawn actor");

        actor_ref.register_handler_sync(
            MessageKey(ConcurrentTestMessageKey::GetState),
            get_state_handler,
        );

        actor_ref.register_handler_sync(
            MessageKey(ConcurrentTestMessageKey::SyncNonMut),
            sync_non_mut_handler,
        );
        actor_ref.register_handler_sync_mutating(
            MessageKey(ConcurrentTestMessageKey::SyncMut),
            sync_mut_handler,
        );

        future::block_on(async move {
            actor_ref
                .tell(&dispatcher, SyncNonMutMessage::default())
                .unwrap();
            actor_ref
                .tell(&dispatcher, SyncNonMutMessage::default())
                .unwrap();
            actor_ref
                .tell(&dispatcher, SyncMutMessage::default())
                .unwrap();
            actor_ref
                .tell(&dispatcher, SyncNonMutMessage::default())
                .unwrap();
            actor_ref
                .tell(&dispatcher, SyncNonMutMessage::default())
                .unwrap();
            actor_ref
                .tell(&dispatcher, SyncMutMessage::default())
                .unwrap();
            actor_ref
                .tell(&dispatcher, SyncNonMutMessage::default())
                .unwrap();

            // Allow time for processing
            smol::Timer::after(Duration::from_millis(50)).await;

            let state: ConcurrencyTestState = actor_ref
                .ask(&dispatcher, GetStateMessage)
                .await
                .expect("Failed to get state");

            let final_order = state.execution_log.lock().await.clone();

            insta::assert_snapshot!(final_order.join("\n"));
        });
    }
}
