use crate::{
    message::{Message, MessageKey, Response},
    types::ActorError,
};
use std::{any::TypeId, fmt::Debug, hash::Hash};

pub trait Dispatcher<K: Eq + Hash + Debug>: Send + Sync + 'static {
    fn message_key(&self, message: &dyn Message) -> Result<MessageKey<K>, ActorError>;

    fn wrap(
        &self,
        message: Box<dyn Message>,
        key: MessageKey<K>,
    ) -> Result<Box<dyn Message>, ActorError>;

    fn unwrap(
        &self,
        message: Box<dyn Response>,
        key: MessageKey<K>,
    ) -> Result<Box<dyn Response>, ActorError>;
}

pub struct GenericDispatcher;

impl Default for GenericDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl GenericDispatcher {
    pub fn new() -> Self {
        Self
    }
}

impl Dispatcher<TypeId> for GenericDispatcher {
    fn message_key(&self, message: &dyn Message) -> Result<MessageKey<TypeId>, ActorError> {
        Ok(MessageKey(message.type_id()))
    }

    fn wrap(
        &self,
        message: Box<dyn Message>,
        _key: MessageKey<TypeId>,
    ) -> Result<Box<dyn Message>, ActorError> {
        Ok(message)
    }

    fn unwrap(
        &self,
        response: Box<dyn Response>,
        _key: MessageKey<TypeId>,
    ) -> Result<Box<dyn Response>, ActorError> {
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        builder::ActorBuilder,
        message::{MessageDowncast, ResponseDowncast},
    };
    // use std::sync::Arc;

    // Test message and response types
    #[derive(Debug, Clone)]
    struct IncrementMessage(i32);

    #[derive(Debug, Clone)]
    struct IncrementResponse(i32);

    #[derive(Debug, Clone)]
    struct GetCounterMessage;

    #[derive(Debug, Clone)]
    struct GetCounterResponse(i32);

    // Implement Message and Response traits for our test types
    // impl Message for IncrementMessage {}
    // impl Response for IncrementResponse {}
    // impl Message for GetCounterMessage {}
    // impl Response for GetCounterResponse {}

    // A simple state to work with
    struct TestState {
        counter: i32,
    }

    #[test]
    fn test_message_key_generation() {
        let dispatcher = GenericDispatcher::new();
        let increment_msg = IncrementMessage(5);
        let get_counter_msg = GetCounterMessage;

        let key1 = dispatcher.message_key(&increment_msg).unwrap();
        let key2 = dispatcher.message_key(&get_counter_msg).unwrap();

        assert_ne!(key1, key2);

        // Ensure keys match expected values
        let expected_key1 = MessageKey(TypeId::of::<IncrementMessage>());
        let expected_key2 = MessageKey(TypeId::of::<GetCounterMessage>());

        assert_eq!(key1, expected_key1);
        assert_eq!(key2, expected_key2);
    }

    #[test]
    fn test_message_wrapping_and_unwrapping() {
        let dispatcher = GenericDispatcher::new();

        let increment_msg = Box::new(IncrementMessage(5)) as Box<dyn Message>;
        let key = dispatcher.message_key(increment_msg.as_ref()).unwrap();

        // Test wrapping
        let wrapped = dispatcher.wrap(increment_msg, key.clone()).unwrap();
        assert!(wrapped.is::<IncrementMessage>());

        // Test unwrapping
        let response = Box::new(IncrementResponse(5)) as Box<dyn Response>;
        let unwrapped = dispatcher.unwrap(response, key).unwrap();
        assert!(unwrapped.is::<IncrementResponse>());
    }

    #[test]
    fn test_integration_with_actor() {
        let actor = ActorBuilder::new()
            .with_state_init(|| Ok(TestState { counter: 0 }))
            .spawn()
            .expect("Failed to spawn actor");

        // Register handlers
        async fn increment_handler(
            state: &mut TestState,
            msg: IncrementMessage,
        ) -> Result<IncrementResponse, ActorError> {
            state.counter += msg.0;
            Ok(IncrementResponse(state.counter))
        }

        async fn get_counter_handler(
            state: &mut TestState,
            _: GetCounterMessage,
        ) -> Result<GetCounterResponse, ActorError> {
            Ok(GetCounterResponse(state.counter))
        }

        // Register handlers with the actor
        actor.register_handler_async_mutating(
            MessageKey(TypeId::of::<IncrementMessage>()),
            increment_handler,
        );
        actor.register_handler_async_mutating(
            MessageKey(TypeId::of::<GetCounterMessage>()),
            get_counter_handler,
        );

        let actor_clone = actor.clone();

        std::thread::spawn(move || {
            smol::block_on(async {
                let dispatcher = GenericDispatcher::new();

                // Increment the counter
                let increment_result: IncrementResponse = actor_clone
                    .ask(&dispatcher, IncrementMessage(5))
                    .await
                    .expect("Failed to increment");
                assert_eq!(increment_result.0, 5);

                // Get the counter value
                let counter_result: GetCounterResponse = actor_clone
                    .ask(&dispatcher, GetCounterMessage)
                    .await
                    .expect("Failed to get counter");
                assert_eq!(counter_result.0, 5);
            });
        })
        .join()
        .expect("Thread panicked");
    }
}
