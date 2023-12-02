use std::collections::HashMap;

use crate::actors::websocket_actor::ChatMessage;
use scylla::{
    prepared_statement::PreparedStatement, query::Query, statement::SerialConsistency, Bytes,
    IntoTypedRows, Session, SessionBuilder,
};
use uuid::Uuid;

use self::data::{ChatInfo, ChatType, UserInfo};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct PageIndex {
    index: Option<Vec<u8>>,
}

impl PageIndex {
    fn from(v: Option<Bytes>) -> PageIndex {
        PageIndex {
            index: v.map_or_else(|| None, |v| Some(v.to_vec())),
        }
    }

    fn into(self) -> Option<Bytes> {
        self.index.map_or_else(|| None, |v| Some(Bytes::from(v)))
    }
}

pub mod data {
    use scylla::frame::response::result::CqlValue;
    use scylla::{
        cql_to_rust::{FromCqlVal, FromCqlValError},
        FromRow,
    };
    use serde::{Deserialize, Serialize};
    use uuid::Uuid;

    #[derive(Debug, Serialize, Deserialize, FromRow)]
    pub struct UserInfo {
        pub id: i64,
        pub name: String,
        pub chats: Vec<Uuid>,
    }

    #[derive(PartialEq, Debug, Serialize, Deserialize)]
    #[serde(tag = "type")]
    pub enum ChatType {
        #[serde(rename = "private")]
        Private,
        #[serde(rename = "group")]
        Group,
        #[serde(rename = "reserved")]
        Reserved,
    }

    impl FromCqlVal<CqlValue> for ChatType {
        fn from_cql(cql_val: CqlValue) -> Result<Self, scylla::cql_to_rust::FromCqlValError> {
            Ok(
                match &*cql_val.into_string().ok_or(FromCqlValError::BadCqlType)? {
                    "group" => ChatType::Group,
                    "private" => ChatType::Private,
                    _ => ChatType::Reserved,
                },
            )
        }
    }

    #[derive(Debug, Serialize, Deserialize, FromRow)]
    pub struct ChatInfo {
        pub id: Uuid,
        pub name: String,
        pub users: Vec<i64>,
        pub chat_type: ChatType,
    }
}

#[derive(Debug)]
pub enum DBError {
    LogicError(Box<dyn std::error::Error + Send>),
    QueryError(Box<dyn std::error::Error + Send>),
    OtherError(Box<dyn std::error::Error + Send>),
}

impl std::fmt::Display for DBError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DBError::LogicError(e) => {
                write!(f, "Logic error: {e}")
            }
            DBError::QueryError(e) => {
                write!(f, "Query error: {e}")
            }
            DBError::OtherError(e) => {
                write!(f, "Other error: {e}")
            }
        }
    }
}

#[derive(Debug)]
struct StringError {
    msg: String,
}

impl std::fmt::Display for StringError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.msg)
    }
}

impl std::error::Error for StringError {
    fn description(&self) -> &str {
        &self.msg
    }
}

pub type DBResult<T> = Result<T, DBError>;

#[mockall::automock]
#[async_trait::async_trait(?Send)]
pub trait Database {
    /// Инициирует базу данных
    async fn init_db(&self) -> DBResult<()>;
    async fn init_db_clear(&self) -> DBResult<()>;
    async fn add_new_message_to_chat(&self, msg: ChatMessage) -> DBResult<()>;
    async fn get_chat_history_paged(
        &self,
        user_id: i64,
        chat_id: uuid::Uuid,
        page_size: usize,
        paging_index: Option<PageIndex>,
    ) -> DBResult<(Vec<ChatMessage>, PageIndex)>;
    async fn create_new_chat(
        &self,
        user_id: i64,
        invited_users_id: Vec<i64>,
        chat_type: data::ChatType,
        chat_name: String,
    ) -> DBResult<data::ChatInfo>;
    async fn add_user_to_chat(
        &self,
        user_id: i64,
        invited_user_id: i64,
        chat_id: uuid::Uuid,
    ) -> DBResult<()>;
    async fn exit_chat(&self, user_id: i64, chat_id: uuid::Uuid) -> DBResult<()>;
    async fn delete_chat(&self, chat_id: uuid::Uuid) -> DBResult<()>;
    async fn get_chat_info(&self, user_id: i64, chat_id: uuid::Uuid) -> DBResult<data::ChatInfo>;
    async fn get_user_info(&self, user_id: i64) -> DBResult<UserInfo>;
    async fn create_new_user(&self, user_id: i64, user_name: String) -> DBResult<UserInfo>;
    async fn get_user_chats(&self, user_id: i64) -> DBResult<Vec<Uuid>>;
    async fn get_user_list(&self) -> DBResult<Vec<i64>>;
}

pub struct ScyllaDatabase {
    pub client: Session,
    prepared_queries: HashMap<String, PreparedStatement>,
    // prepared_transactions: HashMap<String, Batch>
}

impl ScyllaDatabase {
    pub async fn new(host: String, port: u16) -> DBResult<Self> {
        let connection_string = format!("{}:{}", host, port);
        let session: Session = SessionBuilder::new()
            .known_node(connection_string)
            .build()
            .await
            .map_err(|e| DBError::OtherError(Box::new(e)))?;
        Ok(Self {
            client: session,
            prepared_queries: HashMap::new(),
        })
    }

    async fn get_prepared_query(
        &self,
        key: &str,
        query_fallback: &str,
    ) -> DBResult<PreparedStatement> {
        Ok(if let Some(prepared) = self.prepared_queries.get(key) {
            prepared.clone()
        } else {
            let mut q = Query::new(query_fallback);
            q.set_consistency(scylla::statement::Consistency::One);
            q.set_serial_consistency(Some(SerialConsistency::Serial));
            self.client
                .prepare(q)
                .await
                .map_err(|e| DBError::QueryError(Box::new(e)))?
        })
    }
}

#[async_trait::async_trait(?Send)]
impl Database for ScyllaDatabase {
    async fn init_db(&self) -> DBResult<()> {
        let q = self.get_prepared_query("create keyspace", r#"CREATE KEYSPACE IF NOT EXISTS chat WITH replication = {'class': 'NetworkTopologyStrategy', 'replication_factor': 1}"#)
            .await?;

        self.client
            .execute(&q, &[])
            .await
            .map_err(|e| DBError::QueryError(Box::new(e)))?;

        let q = self
            .get_prepared_query(
                "create users table",
                r#"CREATE TABLE IF NOT EXISTS chat.users (
                user_id BIGINT PRIMARY KEY,
                creation_date TIMESTAMP,
                name TEXT,
                chats SET<UUID>)"#,
            )
            .await?;

        self.client
            .execute(&q, &[])
            .await
            .map_err(|e| DBError::QueryError(Box::new(e)))?;

        let q = self
            .get_prepared_query(
                "create chats table",
                r#"CREATE TABLE IF NOT EXISTS chat.chats (
                chat_id UUID PRIMARY KEY,
                creation_date TIMESTAMP,
                name TEXT,
                users SET<BIGINT>,
                chat_type TEXT)"#,
            )
            .await?;

        self.client
            .execute(&q, &[])
            .await
            .map_err(|e| DBError::QueryError(Box::new(e)))?;
        Ok(())
    }
    async fn init_db_clear(&self) -> DBResult<()> {
        let q = self
            .get_prepared_query("drop keyspace", r#"DROP KEYSPACE IF EXISTS chat"#)
            .await?;

        self.client
            .execute(&q, &[])
            .await
            .map_err(|e| DBError::QueryError(Box::new(e)))?;

        let q = self.get_prepared_query("create keyspace", r#"CREATE KEYSPACE IF NOT EXISTS chat WITH replication = {'class': 'NetworkTopologyStrategy', 'replication_factor': 1}"#)
            .await?;

        self.client
            .execute(&q, &[])
            .await
            .map_err(|e| DBError::QueryError(Box::new(e)))?;

        let q = self
            .get_prepared_query(
                "create users table",
                r#"CREATE TABLE IF NOT EXISTS chat.users (
                user_id BIGINT PRIMARY KEY,
                creation_date TIMESTAMP,
                name TEXT,
                chats SET<UUID>)"#,
            )
            .await?;

        self.client
            .execute(&q, &[])
            .await
            .map_err(|e| DBError::QueryError(Box::new(e)))?;

        let q = self
            .get_prepared_query(
                "create chats table",
                r#"CREATE TABLE IF NOT EXISTS chat.chats (
                chat_id UUID PRIMARY KEY,
                creation_date TIMESTAMP,
                name TEXT,
                users SET<BIGINT>,
                chat_type TEXT)"#,
            )
            .await?;

        self.client
            .execute(&q, &[])
            .await
            .map_err(|e| DBError::QueryError(Box::new(e)))?;
        Ok(())
    }
    async fn add_new_message_to_chat(&self, msg: ChatMessage) -> DBResult<()> {
        // Готовим транзакцию для вставки сообщения в чат
        // 1) Проверяем наличие пользователя в чате
        // 2) Проверяем наличие чата у пользователя
        // 3) Всавляем сообщение в чат
        let user_chats = self.get_user_chats(msg.sender_id).await?;
        if !user_chats.contains(&msg.chat_id) {
            return Err(DBError::LogicError(Box::new(StringError {
                msg: "User is not a member of this chat".into(),
            })));
        }
        let i = msg.chat_id.to_string().replace("-", "_");
        let query_name = format!("add msg to chat_{}", i);
        let query_body = format!(
            r#"INSERT INTO chat.chat_{} (message_id, user_id, date, message_text, yes)
        VALUES (uuid(), ?, toTimestamp(now()), ?, true)"#,
            i
        );
        let q = self.get_prepared_query(&query_name, &query_body).await?;

        // Добавляем сообщение в чат
        self.client
            .execute(&q, (msg.sender_id, msg.msg_text))
            .await
            .map_err(|e| DBError::QueryError(Box::new(e)))?;
        Ok(())
    }

    async fn create_new_chat(
        &self,
        user_id: i64,
        mut invited_users_id: Vec<i64>,
        chat_type: data::ChatType,
        chat_name: String,
    ) -> DBResult<data::ChatInfo> {
        invited_users_id.push(user_id);
        let user_list = self.get_user_list().await?;
        let are_invited_users_registered = invited_users_id
            .iter()
            .map(|elem| user_list.contains(elem))
            .all(|elem| elem);

        if !are_invited_users_registered {
            return Err(DBError::LogicError(Box::new(StringError {
                msg: "Invited user is not registered".into(),
            })));
        }

        // Готовим данные о новом чате
        let new_chat_id = Uuid::new_v4();
        let chat_type = match chat_type {
            ChatType::Private => "private",
            ChatType::Group => "group",
            ChatType::Reserved => "reserved",
        };

        // Готовим запрос на добавление информации о новом чате в таблицу чатов

        let q = self
            .get_prepared_query(
                "add new chat info",
                r#"INSERT INTO chat.chats (chat_id, creation_date, name, users, chat_type)
            VALUES (?, toTimestamp(now()), ?, ?, ?)
            IF NOT EXISTS"#,
            )
            .await?;

        // Добавляем информацию о новом чате
        self.client
            .execute(&q, (new_chat_id, chat_name, &invited_users_id, chat_type))
            .await
            .map_err(|e| DBError::QueryError(Box::new(e)))?;

        let q = self
            .get_prepared_query(
                "update users chat lists",
                r#"UPDATE chat.users
            SET chats = chats + {?}
            WHERE user_id IN ?"#,
            )
            .await?;

        self.client
            .execute(&q, (new_chat_id, &invited_users_id))
            .await
            .map_err(|e| DBError::QueryError(Box::new(e)))?;

        let i = new_chat_id.to_string().replace("-", "_");
        let q = format!(
            "CREATE TABLE IF NOT EXISTS chat.chat_{i} \
            (message_id UUID, \
            user_id BIGINT, \
            date TIMESTAMP, \
            message_text TEXT, \
            yes BOOLEAN, \
            PRIMARY KEY (yes, date, message_id)) \
            WITH CLUSTERING ORDER BY (date desc)"
        );

        // Создаем таблицу сообщений нового чата
        self.client
            .query(q, &[])
            .await
            .map_err(|e| DBError::QueryError(Box::new(e)))?;

        // Если всё замечательно, то получаем данные о чате из базы
        let chat_info = self.get_chat_info(user_id, new_chat_id).await?;
        Ok(chat_info)
    }
    async fn add_user_to_chat(
        &self,
        user_id: i64,
        invited_user_id: i64,
        chat_id: uuid::Uuid,
    ) -> DBResult<()> {
        // Проверка приглашенного пользователя на регистрацию
        let user_list = self.get_user_list().await?;
        if !user_list.contains(&invited_user_id) || !user_list.contains(&user_id) {
            return Err(DBError::LogicError(Box::new(StringError {
                msg: "Invited user is not registered".into(),
            })));
        }

        // Проверка наличия чата у пользователя
        let user_chats = self.get_user_chats(user_id).await?;
        if !user_chats.contains(&chat_id) {
            return Err(DBError::LogicError(Box::new(StringError {
                msg: "User is not a member of this chat".into(),
            })));
        }

        let q_1 = self
            .get_prepared_query(
                "add user to chat",
                "UPDATE chat.chats \
             SET users = users + {?} \
             WHERE chat_id = ? \
             IF EXISTS",
            )
            .await?;

        let q_2 = self
            .get_prepared_query(
                "add chat to user",
                "UPDATE chat.users \
             SET chats = chats + {?} \
             WHERE user_id = ? \
             IF EXISTS",
            )
            .await?;

        self.client
            .execute(&q_1, (invited_user_id, chat_id))
            .await
            .map_err(|e| DBError::QueryError(Box::new(e)))?;
        self.client
            .execute(&q_2, (chat_id, invited_user_id))
            .await
            .map_err(|e| DBError::QueryError(Box::new(e)))?;
        Ok(())
    }

    async fn exit_chat(&self, user_id: i64, chat_id: uuid::Uuid) -> DBResult<()> {
        // Готовим транзакцию удаления пользователя
        // 1) Удаляем пользователя из чата
        // 2) Удаляем чат из списка пользователя
        let q_1 = self
            .get_prepared_query(
                "delete user from chat",
                "UPDATE chat.chats \
             SET users = users - {?} \
             WHERE chat_id = ? \
             IF EXISTS",
            )
            .await?;
        let q_2 = self
            .get_prepared_query(
                "delete chat from user",
                "UPDATE chat.users \
             SET chats = chats - {?} \
             WHERE user_id = ? \
             IF EXISTS",
            )
            .await?;

        self.client
            .execute(&q_1, (user_id, chat_id))
            .await
            .map_err(|e| DBError::QueryError(Box::new(e)))?;
        self.client
            .execute(&q_2, (chat_id, user_id))
            .await
            .map_err(|e| DBError::QueryError(Box::new(e)))?;

        // Проверяем, есть ли еще кто-то в данном чате
        // Если нет, то удаляем его

        let q = self
            .get_prepared_query(
                "get chat user count",
                "SELECT users FROM chat.chats WHERE chat_id = ?",
            )
            .await?;
        let chat_user_list = self
            .client
            .execute(&q, (chat_id,))
            .await
            .map_err(|e| DBError::QueryError(Box::new(e)))?
            .rows
            .ok_or(DBError::QueryError(Box::new(StringError {
                msg: "Select query didn't return rows".into(),
            })))?
            .into_typed::<(Option<Vec<i64>>,)>()
            .next()
            .ok_or(DBError::LogicError(Box::new(StringError {
                msg: "Invalid chat ID to delete".into(),
            })))?
            .map_err(|e| DBError::OtherError(Box::new(e)))?
            .0;
        match chat_user_list {
            Some(v) if v.is_empty() => {
                self.delete_chat(chat_id).await?;
            }
            None => {
                self.delete_chat(chat_id).await?;
            }
            _ => {}
        }
        Ok(())
    }
    async fn delete_chat(&self, chat_id: uuid::Uuid) -> DBResult<()> {
        let i = chat_id.to_string().replace("-", "_");
        let q_1 = self
            .get_prepared_query(
                "delete chat record from chats",
                "DELETE FROM chat.chats WHERE chat_id = ? IF EXISTS",
            )
            .await?;
        self.client
            .execute(&q_1, (chat_id,))
            .await
            .map_err(|e| DBError::QueryError(Box::new(e)))?;
        let q_2 = self
            .get_prepared_query(
                "delete chat history",
                format!("DROP TABLE IF EXISTS chat.chat_{}", i).as_str(),
            )
            .await?;
        self.client
            .execute(&q_2, &[])
            .await
            .map_err(|e| DBError::QueryError(Box::new(e)))?;
        Ok(())
    }

    async fn get_chat_info(&self, user_id: i64, chat_id: uuid::Uuid) -> DBResult<data::ChatInfo> {
        let query_body = "SELECT chat_id, name, users, chat_type FROM chat.chats WHERE chat_id = ? AND users CONTAINS ? ALLOW FILTERING";
        let q = self.get_prepared_query("get chat info", query_body).await?;
        let chat_info = self
            .client
            .execute(&q, (chat_id, user_id))
            .await
            .map_err(|e| DBError::QueryError(Box::new(e)))?
            .rows
            .ok_or(DBError::QueryError(Box::new(StringError {
                msg: "Select query didn't return rows".into(),
            })))?
            .into_typed::<(Uuid, String, Option<Vec<i64>>, ChatType)>()
            .next()
            .ok_or(DBError::LogicError(Box::new(StringError {
                msg: "Invalid chat ID or User is not a member of chat".into(),
            })))?
            .map_err(|e| DBError::OtherError(Box::new(e)))?;
        Ok(ChatInfo {
            id: chat_info.0,
            name: chat_info.1,
            users: chat_info.2.unwrap_or(vec![]),
            chat_type: chat_info.3,
        })
    }
    async fn get_chat_history_paged(
        &self,
        user_id: i64,
        chat_id: uuid::Uuid,
        page_size: usize,
        paging_index: Option<PageIndex>,
    ) -> DBResult<(Vec<ChatMessage>, PageIndex)> {
        // Чтобы получить сообщения чата, необходимо:
        // 1) Проверить, есть ли пользователь в чате
        // 2) Получить только часть данных
        // 3) Отправить ее
        let user_chats = self.get_user_chats(user_id).await?;
        if !user_chats.contains(&chat_id) {
            Err(DBError::LogicError(Box::new(StringError {
                msg: "User is not a member of chat".into(),
            })))?;
        }
        let i = chat_id.to_string().replace("-", "_");
        let query_name = format!("get chat_{} messages", i);
        let query_body = format!(r#"SELECT user_id, date, message_text FROM chat.chat_{}"#, i);
        let mut q = self.get_prepared_query(&query_name, &query_body).await?;
        q.set_page_size(page_size as i32);

        let current_page = if let Some(index) = paging_index {
            let paging_index: Option<Bytes> = index.into();
            self.client
                .execute_paged(&q, &[], paging_index)
                .await
                .map_err(|e| DBError::QueryError(Box::new(e)))?
        } else {
            self.client
                .execute(&q, &[])
                .await
                .map_err(|e| DBError::QueryError(Box::new(e)))?
        };

        let next_index = PageIndex::from(current_page.paging_state);
        // let next_index = to_string(&next_index).unwrap();

        let messages: Result<Vec<_>, _> = current_page
            .rows
            .ok_or(DBError::QueryError(Box::new(StringError {
                msg: "Select query didn't rerurn rows".into(),
            })))?
            .into_typed::<(i64, chrono::Duration, String)>()
            .collect();
        let messages: Vec<_> = messages
            .map_err(|e| DBError::OtherError(Box::new(e)))?
            .into_iter()
            .map(|msg| ChatMessage {
                chat_id,
                date: msg.1.into(),
                sender_id: msg.0,
                msg_text: msg.2,
            })
            .collect();
        Ok((messages, next_index))
    }
    async fn get_user_info(&self, user_id: i64) -> DBResult<UserInfo> {
        let q = self
            .get_prepared_query(
                "get user info",
                r#"SELECT user_id, name, chats from chat.users WHERE user_id = ?"#,
            )
            .await?;
        let user_info = self
            .client
            .execute(&q, (user_id,))
            .await
            .map_err(|e| DBError::QueryError(Box::new(e)))?
            .rows
            .ok_or(DBError::QueryError(Box::new(StringError {
                msg: "Select query didn't rerurn rows".into(),
            })))?
            .into_typed::<(i64, String, Option<Vec<Uuid>>)>()
            .next()
            .ok_or(DBError::LogicError(Box::new(StringError {
                msg: "Invalid User ID".into(),
            })))?
            .map_err(|e| DBError::OtherError(Box::new(e)))?;
        Ok(UserInfo {
            id: user_info.0,
            name: user_info.1,
            chats: user_info.2.unwrap_or(vec![]),
        })
    }
    async fn create_new_user(&self, user_id: i64, user_name: String) -> DBResult<UserInfo> {
        let q = self
            .get_prepared_query(
                "create new user",
                r#"INSERT INTO chat.users (user_id, creation_date, name, chats)
               VALUES (?, toTimestamp(now()), ?, {})
               IF NOT EXISTS"#,
            )
            .await?;
        self.client
            .execute(&q, (user_id, user_name))
            .await
            .map_err(|e| DBError::QueryError(Box::new(e)))?;
        let user_info = self.get_user_info(user_id).await?;
        Ok(user_info)
    }
    async fn get_user_chats(&self, user_id: i64) -> DBResult<Vec<Uuid>> {
        let q = self
            .get_prepared_query(
                "get user chats",
                r#"SELECT chats FROM chat.users WHERE user_id = ?"#,
            )
            .await?;
        let chats = self
            .client
            .execute(&q, (user_id,))
            .await
            .map_err(|e| DBError::QueryError(Box::new(e)))?
            .rows
            .ok_or(DBError::QueryError(Box::new(StringError {
                msg: "Select query didn't return rows".into(),
            })))?
            .into_typed::<(Option<Vec<Uuid>>,)>()
            .next()
            .ok_or(DBError::LogicError(Box::new(StringError {
                msg: "Invalid user id".into(),
            })))?
            .map_err(|e| DBError::OtherError(Box::new(e)))?
            .0;
        Ok(chats.unwrap_or(vec![]))
    }

    async fn get_user_list(&self) -> DBResult<Vec<i64>> {
        let q = self
            .get_prepared_query("get user list", r#"SELECT user_id FROM chat.users"#)
            .await?;
        let user_list: Result<Vec<_>, _> = self
            .client
            .execute(&q, &[])
            .await
            .map_err(|e| DBError::QueryError(Box::new(e)))?
            .rows_typed_or_empty::<(i64,)>()
            .map(|elem| match elem {
                Ok(id) => Ok(id.0),
                Err(e) => Err(e),
            })
            .collect();
        let user_list = user_list.map_err(|e| DBError::OtherError(Box::new(e)))?;
        Ok(user_list)
    }
}
