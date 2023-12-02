use crate::actors::websocket_actor::ChatMessage;
use actix::prelude::*;
use futures_util::StreamExt;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use std::{error::Error, sync::Arc};
use tokio::sync::Mutex;
use uuid::Uuid;

use super::broker_actor::{self, BrokerActor};

#[derive(Serialize, Deserialize)]
pub struct SubscriptionData {
    pub chat_id: Uuid,
    pub user_id: i64,
}

// Какие сообщения принимает
pub mod messages {
    use super::*;

    #[derive(Message)]
    #[rtype(result = "()")]
    pub enum ApiMessage {
        NewSubscription(SubscriptionData),
        NewUnsubscription(SubscriptionData),
    }

    #[derive(Message)]
    #[rtype(result = "()")]
    pub enum WebsocketMessage {
        NewMessage(ChatMessage),
    }
}

pub struct RedisActor {
    client: Arc<Mutex<redis::Client>>,
    connection: Arc<Mutex<redis::aio::Connection>>,
    broker: Addr<BrokerActor>,
}

impl RedisActor {
    pub async fn new(
        host: &str,
        port: u16,
        broker: Addr<BrokerActor>,
    ) -> Result<Self, Box<dyn Error>> {
        let con_str = format!("redis://{}:{}", host, port);
        let client = redis::Client::open(con_str)?;
        let connection = client.get_async_connection().await?;
        let connection = Arc::new(Mutex::new(connection));
        let client = Arc::new(Mutex::new(client));
        Ok(RedisActor {
            connection,
            client,
            broker,
        })
    }
}

impl Actor for RedisActor {
    type Context = Context<Self>;
    fn started(&mut self, ctx: &mut Self::Context) {
        let client = self.client.clone();

        let broker = self.broker.clone();
        Box::pin(async move {
            let receiver = client.lock().await.get_async_connection().await.unwrap();
            // Делаем ресивер из подключения
            let mut receiver = receiver.into_pubsub();

            // Подписываем ресивер на чаты, подписки и отписки
            receiver.subscribe("chat_message").await.unwrap();
            receiver.subscribe("subscribe").await.unwrap();
            receiver.subscribe("unsubscribe").await.unwrap();

            // Получаем поток из ресивера
            let mut stream = receiver.on_message();

            // Бесконечный цикл обработки сообщений:
            // Если получили новое сообщение
            while let Some(msg) = stream.next().await {
                // Получаем название канала и текст сообщения
                let channel: String = msg.get_channel_name().to_owned();
                let text: String = msg.get_payload().unwrap();

                // Делаем разные вещи относительно названия канала
                match channel.as_str() {
                    // Канал подписывания на чаты
                    "subscribe" => {
                        if let Ok(new_sub) = serde_json::from_str::<SubscriptionData>(&text) {
                            broker.do_send(broker_actor::messages::RedisMessage::NewSubscription(
                                new_sub,
                            ));
                        }
                    }
                    // Канал отписывания от чата
                    "unsibscribe" => {
                        if let Ok(new_unsub) = serde_json::from_str::<SubscriptionData>(&text) {
                            broker.do_send(
                                broker_actor::messages::RedisMessage::NewUnsubscription(new_unsub),
                            );
                        }
                    }
                    // Канал сообщений чатов
                    "chat_message" => {
                        if let Ok(new_msg) = serde_json::from_str::<ChatMessage>(&text) {
                            broker
                                .do_send(broker_actor::messages::RedisMessage::NewMessage(new_msg));
                        }
                    }
                    _ => {}
                }
            }
        })
        .into_actor(self)
        .spawn(ctx);
    }
}

impl Handler<messages::WebsocketMessage> for RedisActor {
    type Result = ResponseFuture<()>;
    fn handle(
        &mut self,
        msg: messages::WebsocketMessage,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        let con = self.connection.clone();
        Box::pin(async move {
            match msg {
                messages::WebsocketMessage::NewMessage(new_msg) => {
                    let _ = con
                        .lock()
                        .await
                        .publish::<_, _, String>(
                            "chat_message",
                            serde_json::to_string(&new_msg).unwrap(),
                        )
                        .await;
                }
            }
        })
    }
}
