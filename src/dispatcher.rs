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

// TODO: in the future maybe we can type the dispatcher with its message and response types
// use crate::{
//     message::{Message, MessageKey, Response},
//     types::ActorError,
// };

// pub trait Dispatcher<MessageTyped, ResponseTyped>: Send + Sync + 'static {
//     fn message_key(&self, message: &dyn Message) -> Result<MessageKey, ActorError>;

//     fn wrap(
//         &self,
//         message: Box<MessageTyped>,
//         key: MessageKey,
//     ) -> Result<Box<dyn Message>, ActorError>;

//     fn unwrap(
//         &self,
//         message: Box<dyn Response>,
//         key: MessageKey,
//     ) -> Result<ResponseTyped, ActorError>;
