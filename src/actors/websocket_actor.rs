use crate::{
    actors::broker_actor::{self, BrokerActor},
    actors::redis_actor::{self, RedisActor},
    serializable_duration::SerializableDuration,
};
use actix::prelude::*;
use actix_web_actors::ws;
use scylla::FromRow;
use serde::{Deserialize, Serialize};
use serde_json::{from_str, to_string};
use uuid::Uuid;

use super::database_actor::{self, DatabaseActor};

// Когда пользователь пытается подключиться к чату, он отдает свой токен
// Токен проверяется и из него берется id пользователя
// Пытаемся найти данный id в базе и если находим, то просто отдаем сокет
// Если не находим, то сначала создаем новую запись пользователя и только потом отдаем сокет
//

// Сокет актор имеет следующие свойства:
// 1) Принимает от пользователя NewChatMessage, добавляя к нему свой id и время, получая
//    ChatMessage
// 2) Отправляет ChatMessage в Redis-actor и Database-actor

#[derive(Serialize, Deserialize, FromRow, Clone)]
pub struct ChatMessage {
    pub chat_id: Uuid,
    pub sender_id: i64,
    pub date: SerializableDuration,
    pub msg_text: String,
}

#[derive(Serialize, Deserialize)]
pub struct NewChatMessage {
    chat_id: Uuid,
    msg_text: String,
}

// Какие сообщения принимает
pub mod messages {
    use super::*;

    #[derive(Message)]
    #[rtype(result = "()")]
    pub enum BrokerMessage {
        NewMessage(ChatMessage),
    }
}

pub struct WebsocketActor {
    broker: Addr<BrokerActor>,
    publisher: Addr<RedisActor>,
    db: Addr<DatabaseActor>,
    user_id: i64,
}

impl WebsocketActor {
    pub fn new(
        broker: Addr<BrokerActor>,
        publisher: Addr<RedisActor>,
        db: Addr<DatabaseActor>,
        user_id: i64,
    ) -> Self {
        Self {
            broker,
            publisher,
            db,
            user_id,
        }
    }
}

impl Actor for WebsocketActor {
    type Context = ws::WebsocketContext<Self>;
    fn started(&mut self, ctx: &mut Self::Context) {
        self.broker.do_send(
            broker_actor::messages::WebsocketMessage::BrokerNotifyStarted(
                ctx.address(),
                self.user_id,
            ),
        );
    }
    fn stopped(&mut self, ctx: &mut Self::Context) {
        self.broker.do_send(
            broker_actor::messages::WebsocketMessage::BrokerNotifyClosed(
                ctx.address(),
                self.user_id,
            ),
        );
    }
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for WebsocketActor {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        match msg {
            // Получаем текст по вебсокету
            Ok(ws::Message::Text(text)) => {
                // Приводим его к типу "Новое сообщение"
                let user_msg: NewChatMessage = from_str(&text).unwrap();

                // Из нового сообщения состряпываем нормальное с нужными данными
                let chat_msg = ChatMessage {
                    chat_id: user_msg.chat_id,
                    sender_id: self.user_id,
                    date: (chrono::Utc::now() - chrono::DateTime::UNIX_EPOCH).into(),
                    msg_text: user_msg.msg_text,
                };

                // Отправляем сообщение в базу, не так важно, если оно не дошло
                self.db
                    .do_send(database_actor::messages::InsertNewMessage(chat_msg.clone()));

                // Отправляем сообщение в редис-брокер, не так важно, если не дошло
                self.publisher
                    .do_send(redis_actor::messages::WebsocketMessage::NewMessage(
                        chat_msg,
                    ));
            }
            Ok(ws::Message::Close(_)) => ctx.stop(),
            _ => (),
        }
    }
}

impl Handler<messages::BrokerMessage> for WebsocketActor {
    type Result = ();
    fn handle(&mut self, msg: messages::BrokerMessage, ctx: &mut Self::Context) -> Self::Result {
        match msg {
            messages::BrokerMessage::NewMessage(new_msg) => {
                let m = to_string(&new_msg).unwrap();
                ctx.text(m);
            }
        }
    }
}
