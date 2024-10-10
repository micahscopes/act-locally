use std::{
    error::Error,
    fmt::{self, Debug},
    hash::Hash,
};

use crate::{
    actor::ActorRef,
    dispatcher::Dispatcher,
    message::{Message, MessageDowncast, MessageKey, Response},
    types::ActorError,
};

#[allow(unused)]
pub struct ActionDispatcher<S, K>
where
    K: Eq + Hash + Debug + Clone,
{
    key: MessageKey<K>,
    actor: ActorRef<S, K>,
}
pub struct ActionError(pub Box<dyn Error + Send + Sync>);
impl fmt::Debug for ActionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl fmt::Display for ActionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl Error for ActionError {}

pub type ActionFunc<S> = Box<dyn FnOnce(&mut S) -> Result<(), ActionError> + Send>;

#[allow(unused)]
impl<S: 'static, K: Eq + Hash + Debug + Clone + Send + Sync + 'static> ActionDispatcher<S, K> {
    // Register a new dispatcher with the actor
    pub fn register(key: MessageKey<K>, actor: ActorRef<S, K>) -> Self {
        let handler = move |state: &mut S, func: ActionFunc<S>| func(state);
        // Register the handler with the actor
        actor.register_handler_sync_mutating::<ActionFunc<S>, (), ActionError, _>(
            key.clone(),
            Box::new(handler),
        );

        Self { key, actor }
    }

    pub fn act(&self, func: ActionFunc<S>) -> Result<(), ActorError> {
        self.actor.tell(self, func)
    }
}

impl<S: 'static, K: Eq + Hash + Debug + Clone + Send + Sync + 'static> Dispatcher<K>
    for ActionDispatcher<S, K>
{
    fn message_key(&self, message: &dyn Message) -> Result<MessageKey<K>, ActorError> {
        if message.downcast_ref::<ActionFunc<S>>().is_some() {
            Ok(self.key.clone())
        } else {
            Err(ActorError::HandlerNotFound)
        }
    }

    fn wrap(
        &self,
        message: Box<dyn Message>,
        key: MessageKey<K>,
    ) -> Result<Box<dyn Message>, ActorError> {
        if key == self.key {
            Ok(message)
        } else {
            Err(ActorError::HandlerNotFound)
        }
    }

    fn unwrap(
        &self,
        message: Box<dyn Response>,
        key: MessageKey<K>,
    ) -> Result<Box<dyn Response>, ActorError> {
        if key == self.key {
            Ok(message)
        } else {
            Err(ActorError::HandlerNotFound)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action_dispatcher::ActionDispatcher;
    use crate::builder::ActorBuilder;
    use crate::message::MessageKey;

    // Define the actor's state
    #[derive(Debug, Default)]
    struct TestState {
        counter: i32,
    }

    #[test]
    fn test_action_dispatcher() {
        // Initialize the actor with TestState
        let actor_ref = ActorBuilder::new()
            .with_state_init(|| Ok(TestState { counter: 0 }))
            .spawn()
            .expect("Failed to spawn actor");

        // Register the ActionDispatcher with the actor
        let action_dispatcher = ActionDispatcher::register(MessageKey("action"), actor_ref.clone());

        // Define some actions
        let increment_by_5: ActionFunc<TestState> = Box::new(|state| {
            state.counter += 5;
            assert_eq!(state.counter, 5);
            Ok(())
        });

        let increment_by_10: ActionFunc<TestState> = Box::new(|state| {
            state.counter += 10;
            assert_eq!(state.counter, 15);
            Ok(())
        });

        // Perform actions
        action_dispatcher
            .act(increment_by_5)
            .expect("Failed to perform action");
        action_dispatcher
            .act(increment_by_10)
            .expect("Failed to perform action");

        println!("ActionDispatcher test passed successfully!");
    }
}
