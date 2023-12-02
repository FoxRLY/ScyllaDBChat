use crate::actors::database_actor;
use crate::{
    actors::websocket_actor::{self, ChatMessage, WebsocketActor},
    database::DBResult,
};
use actix::prelude::*;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use tokio::sync::Mutex;
use uuid::Uuid;

use super::database_actor::DatabaseActor;

// Что должен делать Брокер?
// 1) Принимать сообщения от Редис-актора
// 2) Отдавать полученные сообщения по сокетам
// 3) Принимать запросы на подписку пользователя на определенные чаты
// 4) Принимать запросы на отписку пользователя от определенныъ чатов
//
//
// Когда пользователь подключается к чату, брокер получает список всех чатов пользователей и
// обновляет свою таблицу: добавляет в socket_map новый id пользователя(если его не было раньше) с
// сокетом и обновляет subscribers, добавляя пользователя в каналы

type AsyncMutex<T> = Arc<Mutex<T>>;

// Какие сообщения принимает
pub mod messages {
    use crate::actors::redis_actor::SubscriptionData;

    use super::*;

    #[derive(Message)]
    #[rtype(result = "()")]
    pub enum RedisMessage {
        NewMessage(ChatMessage),
        NewSubscription(SubscriptionData),
        NewUnsubscription(SubscriptionData),
    }

    #[derive(Message)]
    #[rtype(result = "()")]
    pub enum WebsocketMessage {
        BrokerNotifyStarted(Addr<WebsocketActor>, i64),
        BrokerNotifyClosed(Addr<WebsocketActor>, i64),
    }
}

pub struct BrokerActor {
    subscribers: AsyncMutex<HashMap<Uuid, HashSet<i64>>>,
    socket_map: AsyncMutex<HashMap<i64, HashSet<Addr<WebsocketActor>>>>,
    db: Addr<DatabaseActor>,
}

impl BrokerActor {
    pub async fn new(db: Addr<DatabaseActor>) -> Self {
        let subscribers = Arc::new(Mutex::new(HashMap::new()));
        let socket_map = Arc::new(Mutex::new(HashMap::new()));
        Self {
            db,
            subscribers,
            socket_map,
        }
    }
}

impl Actor for BrokerActor {
    type Context = Context<Self>;
}

impl Handler<messages::WebsocketMessage> for BrokerActor {
    type Result = ResponseFuture<()>;
    fn handle(
        &mut self,
        msg: messages::WebsocketMessage,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        let subscribers = self.subscribers.clone();
        let socket_map = self.socket_map.clone();
        let db = self.db.clone();
        Box::pin(async move {
            match msg {
                messages::WebsocketMessage::BrokerNotifyStarted(addr, id) => {
                    socket_map
                        .lock()
                        .await
                        .entry(id)
                        .and_modify(|set| {
                            set.insert(addr.clone());
                        })
                        .or_insert({
                            let mut h = HashSet::new();
                            h.insert(addr);
                            h
                        });
                    let user_chats: DBResult<Vec<Uuid>> = db
                        .send(database_actor::messages::GetUserChats { user_id: id })
                        .await
                        .unwrap();
                    if let Ok(chats) = user_chats {
                        for chat in chats {
                            subscribers
                                .lock()
                                .await
                                .entry(chat)
                                .and_modify(|v| {
                                    v.insert(id);
                                })
                                .or_insert({
                                    let mut h = HashSet::new();
                                    h.insert(id);
                                    h
                                });
                        }
                    }
                }
                messages::WebsocketMessage::BrokerNotifyClosed(addr, id) => {
                    socket_map.lock().await.entry(id).and_modify(|set| {
                        set.remove(&addr);
                    });
                }
            }
        })
    }
}

impl Handler<messages::RedisMessage> for BrokerActor {
    type Result = ResponseFuture<()>;
    fn handle(&mut self, msg: messages::RedisMessage, _ctx: &mut Self::Context) -> Self::Result {
        let subscribers = self.subscribers.clone();
        let socket_map = self.socket_map.clone();
        Box::pin(async move {
            match msg {
                messages::RedisMessage::NewMessage(new_msg) => {
                    if let Some(user_ids) = subscribers.lock().await.get(&new_msg.chat_id) {
                        for id in user_ids {
                            if let Some(user_addresses) = socket_map.lock().await.get(id) {
                                for addr in user_addresses {
                                    addr.do_send(
                                        websocket_actor::messages::BrokerMessage::NewMessage(
                                            new_msg.clone(),
                                        ),
                                    );
                                }
                            }
                        }
                    }
                }
                messages::RedisMessage::NewSubscription(sub_data) => {
                    subscribers
                        .lock()
                        .await
                        .entry(sub_data.chat_id)
                        .and_modify(|set| {
                            set.insert(sub_data.user_id);
                        })
                        .or_insert({
                            let mut h = HashSet::new();
                            h.insert(sub_data.user_id);
                            h
                        });
                }
                messages::RedisMessage::NewUnsubscription(sub_data) => {
                    subscribers
                        .lock()
                        .await
                        .entry(sub_data.chat_id)
                        .and_modify(|set| {
                            set.remove(&sub_data.user_id);
                        });
                }
            }
        })
    }
}
