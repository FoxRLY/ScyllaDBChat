use actix::prelude::*;
use std::sync::Arc;

use crate::database::{
    data::{ChatInfo, ChatType, UserInfo},
    DBError, DBResult, Database, PageIndex,
};
use uuid::Uuid;

use super::websocket_actor::ChatMessage;

// База данных должна уметь:
// 1) Создавать новых пользователей                 +
// 2) Получать данные о пользователе                +
// 3) Создавать новые чаты                          +
// 4) Добавлять в чаты новых пользователей          +
// 5) Убирать пользователей из чатов по их желанию  +
// 6) Выдавать информацию о чате                    +
// 7) Выдавать историю сообщений чата               +

pub mod messages {
    use crate::actors::websocket_actor::ChatMessage;
    use crate::database::data::{ChatInfo, UserInfo};
    use crate::database::{DBResult, PageIndex};
    use actix::Message;
    use uuid::Uuid;

    #[derive(Message)]
    #[rtype(result = "DBResult<()>")]
    pub struct InitDatabase;

    #[derive(Message)]
    #[rtype(result = "DBResult<()>")]
    pub struct InitDatabaseClear;

    #[derive(Message)]
    #[rtype(result = "DBResult<()>")]
    pub struct InsertNewMessage(pub ChatMessage);

    #[derive(Message)]
    #[rtype(result = "DBResult<UserInfo>")]
    pub struct GetUserInfo {
        pub user_id: i64,
    }

    #[derive(Message)]
    #[rtype(result = "DBResult<Vec<Uuid>>")]
    pub struct GetUserChats {
        pub user_id: i64,
    }

    #[derive(Message)]
    #[rtype(result = "DBResult<UserInfo>")]
    pub struct CreateNewUser {
        pub user_id: i64,
        pub user_name: String,
    }

    #[derive(Message)]
    #[rtype(result = "DBResult<ChatInfo>")]
    pub struct CreateNewPrivateChat {
        pub creator_id: i64,
        pub chat_name: String,
        pub invited_user_id: i64,
    }

    #[derive(Message)]
    #[rtype(result = "DBResult<ChatInfo>")]
    pub struct CreateNewGroupChat {
        pub creator_id: i64,
        pub invited_users_id: Vec<i64>,
        pub chat_name: String,
    }

    #[derive(Message)]
    #[rtype(result = "DBResult<ChatInfo>")]
    pub struct GetChatInfo {
        pub user_id: i64,
        pub chat_id: Uuid,
    }

    #[derive(Message)]
    #[rtype(result = "DBResult<()>")]
    pub struct InviteUserToChat {
        pub user_id: i64,
        pub chat_id: Uuid,
        pub guest_user_id: i64,
    }

    #[derive(Message)]
    #[rtype(result = "DBResult<()>")]
    pub struct ExitChat {
        pub user_id: i64,
        pub chat_id: Uuid,
    }

    #[derive(Message)]
    #[rtype(result = "DBResult<(Vec<ChatMessage>, PageIndex)>")]
    pub struct GetChatHistory {
        pub user_id: i64,
        pub chat_id: Uuid,
        pub page_index: Option<PageIndex>,
        pub page_size: usize,
    }
}

pub struct DatabaseActor {
    db: Arc<Box<dyn Database>>,
}

impl DatabaseActor {
    pub async fn new(host: String, port: u16) -> Result<Self, DBError> {
        let db = crate::database::ScyllaDatabase::new(host, port).await?;
        let db: Arc<Box<dyn Database>> = Arc::new(Box::new(db));
        Ok(Self { db })
    }
}

impl Actor for DatabaseActor {
    type Context = Context<Self>;
}

impl Handler<messages::InsertNewMessage> for DatabaseActor {
    type Result = ResponseFuture<DBResult<()>>;
    fn handle(
        &mut self,
        msg: messages::InsertNewMessage,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        let db = self.db.clone();
        Box::pin(async move { db.add_new_message_to_chat(msg.0).await })
    }
}

impl Handler<messages::GetUserInfo> for DatabaseActor {
    type Result = ResponseFuture<DBResult<UserInfo>>;
    fn handle(&mut self, msg: messages::GetUserInfo, _ctx: &mut Self::Context) -> Self::Result {
        let db = self.db.clone();
        Box::pin(async move { db.get_user_info(msg.user_id).await })
    }
}

impl Handler<messages::GetUserChats> for DatabaseActor {
    type Result = ResponseFuture<DBResult<Vec<Uuid>>>;
    fn handle(&mut self, msg: messages::GetUserChats, _ctx: &mut Self::Context) -> Self::Result {
        let db = self.db.clone();
        Box::pin(async move { db.get_user_chats(msg.user_id).await })
    }
}

impl Handler<messages::CreateNewUser> for DatabaseActor {
    type Result = ResponseFuture<DBResult<UserInfo>>;

    fn handle(&mut self, msg: messages::CreateNewUser, _ctx: &mut Self::Context) -> Self::Result {
        let db = self.db.clone();
        Box::pin(async move { db.create_new_user(msg.user_id, msg.user_name).await })
    }
}

impl Handler<messages::CreateNewPrivateChat> for DatabaseActor {
    type Result = ResponseFuture<DBResult<ChatInfo>>;
    fn handle(
        &mut self,
        msg: messages::CreateNewPrivateChat,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        let db = self.db.clone();
        Box::pin(async move {
            db.create_new_chat(
                msg.creator_id,
                vec![msg.invited_user_id],
                ChatType::Private,
                msg.chat_name,
            )
            .await
        })
    }
}

impl Handler<messages::CreateNewGroupChat> for DatabaseActor {
    type Result = ResponseFuture<DBResult<ChatInfo>>;
    fn handle(
        &mut self,
        msg: messages::CreateNewGroupChat,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        let db = self.db.clone();
        Box::pin(async move {
            db.create_new_chat(
                msg.creator_id,
                msg.invited_users_id,
                ChatType::Group,
                msg.chat_name,
            )
            .await
        })
    }
}

impl Handler<messages::GetChatInfo> for DatabaseActor {
    type Result = ResponseFuture<DBResult<ChatInfo>>;
    fn handle(&mut self, msg: messages::GetChatInfo, _ctx: &mut Self::Context) -> Self::Result {
        let db = self.db.clone();
        Box::pin(async move { db.get_chat_info(msg.user_id, msg.chat_id).await })
    }
}

impl Handler<messages::InviteUserToChat> for DatabaseActor {
    type Result = ResponseFuture<DBResult<()>>;
    fn handle(
        &mut self,
        msg: messages::InviteUserToChat,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        let db = self.db.clone();
        Box::pin(async move {
            db.add_user_to_chat(msg.user_id, msg.guest_user_id, msg.chat_id)
                .await
        })
    }
}

impl Handler<messages::ExitChat> for DatabaseActor {
    type Result = ResponseFuture<DBResult<()>>;
    fn handle(&mut self, msg: messages::ExitChat, _ctx: &mut Self::Context) -> Self::Result {
        let db = self.db.clone();
        Box::pin(async move { db.exit_chat(msg.user_id, msg.chat_id).await })
    }
}

impl Handler<messages::GetChatHistory> for DatabaseActor {
    type Result = ResponseFuture<DBResult<(Vec<ChatMessage>, PageIndex)>>;
    fn handle(&mut self, msg: messages::GetChatHistory, _ctx: &mut Self::Context) -> Self::Result {
        let db = self.db.clone();
        Box::pin(async move {
            db.get_chat_history_paged(msg.user_id, msg.chat_id, msg.page_size, msg.page_index)
                .await
        })
    }
}

impl Handler<messages::InitDatabase> for DatabaseActor {
    type Result = ResponseFuture<DBResult<()>>;
    fn handle(&mut self, _msg: messages::InitDatabase, _ctx: &mut Self::Context) -> Self::Result {
        let db = self.db.clone();
        Box::pin(async move { db.init_db().await })
    }
}

impl Handler<messages::InitDatabaseClear> for DatabaseActor {
    type Result = ResponseFuture<DBResult<()>>;
    fn handle(
        &mut self,
        _msg: messages::InitDatabaseClear,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        let db = self.db.clone();
        Box::pin(async move { db.init_db_clear().await })
    }
}
