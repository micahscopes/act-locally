use crate::{
    message::{Message, MessageKey, Response},
    types::ActorError,
};

pub trait Dispatcher: Send + Sync + 'static {
    fn message_key(&self, message: &dyn Message) -> Result<MessageKey, ActorError>;

    fn wrap(
        &self,
        message: Box<dyn Message>,
        key: MessageKey,
    ) -> Result<Box<dyn Message>, ActorError>;

    fn unwrap(
        &self,
        message: Box<dyn Response>,
        key: MessageKey,
    ) -> Result<Box<dyn Response>, ActorError>;
}
