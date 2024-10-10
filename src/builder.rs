use std::{fmt::Debug, hash::Hash};

use tracing::subscriber::DefaultGuard;

use crate::{
    actor::{Actor, ActorRef},
    types::ActorError,
};

pub struct ActorBuilder<S, K> {
    pub(crate) init_state: Option<Box<dyn FnOnce() -> Result<S, ActorError> + Send + 'static>>,
    pub(crate) init_subscriber: Option<Box<dyn FnOnce() -> Option<DefaultGuard> + Send + 'static>>,
    pub(crate) name: Option<String>,
    phantom: std::marker::PhantomData<K>,
}

impl<S, K> ActorBuilder<S, K>
where
    S: 'static,
    K: Eq + Hash + Debug + Clone + Send + Sync + 'static,
{
    /// Creates a new `ActorBuilder` instance.
    pub fn new() -> Self {
        Self {
            init_state: None,
            init_subscriber: None,
            name: None,
            phantom: std::marker::PhantomData::<K>,
        }
    }

    /// Sets the state initialization function.
    pub fn with_state_init<F>(mut self, init: F) -> Self
    where
        F: FnOnce() -> Result<S, ActorError> + Send + 'static,
    {
        self.init_state = Some(Box::new(init));
        self
    }

    /// Sets the subscriber initialization function.
    pub fn with_subscriber_init<F>(mut self, init_sub: F) -> Self
    where
        F: FnOnce() -> Option<DefaultGuard> + Send + 'static,
    {
        self.init_subscriber = Some(Box::new(init_sub));
        self
    }

    /// Sets the actor's name.
    pub fn with_name(mut self, name: String) -> Self {
        self.name = Some(name);
        self
    }

    /// Spawns the actor with the configured settings.
    pub fn spawn(self) -> Result<ActorRef<S, K>, ActorError> {
        Actor::spawn_builder(self)
    }
}
