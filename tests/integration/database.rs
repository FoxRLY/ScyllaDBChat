#[cfg(test)]
mod tests {
    use chat::actors::websocket_actor::ChatMessage;
    use chat::database::data::ChatType;
    use chat::database::{Database, ScyllaDatabase};
    use chat::serializable_duration::SerializableDuration;
    use chrono::Duration;
    use scylla::{FromRow, Session};
    use serial_test::serial;
    use std::error::Error;
    use testcontainers::clients::Cli;
    use testcontainers::core::WaitFor;
    use testcontainers::GenericImage;
    use uuid::Uuid;

    #[derive(FromRow)]
    struct ChatsRow {
        chat_id: Uuid,
        creation_date: SerializableDuration,
        name: String,
        users: Option<Vec<i64>>,
        chat_type: String,
    }

    #[derive(FromRow)]
    struct UsersRow {
        user_id: i64,
        creation_date: SerializableDuration,
        name: String,
        chats: Option<Vec<Uuid>>,
    }

    #[derive(FromRow)]
    struct MessageRow {
        message_id: Uuid,
        user_id: i64,
        date: SerializableDuration,
        message_text: String,
    }

    async fn insert_data_into_chats(
        client: &Session,
        chat_name: &str,
        users: Vec<i64>,
        chat_type: &str,
    ) -> Result<(), Box<dyn Error>> {
        if users.is_empty() {
            client
                .query(
                    r#"INSERT INTO chat.chats (chat_id, creation_date, name, users, chat_type) VALUES
                        (
                            uuid(),
                            toTimestamp(now()),
                            ?,
                            {},
                            ?
                    )"#,
                    (chat_name, chat_type,))
                .await?;
        } else {
            client
                .query(
                    r#"INSERT INTO chat.chats (chat_id, creation_date, name, users, chat_type) VALUES
                        (
                            uuid(),
                            toTimestamp(now()),
                            ?,
                            ?,
                            ?
                    )"#,
                    (chat_name, users, chat_type,))
                .await?;
        }
        Ok(())
    }

    async fn insert_data_into_users(
        client: &Session,
        user_id: i64,
        user_name: &str,
        chats: Vec<Uuid>,
    ) -> Result<(), Box<dyn Error>> {
        if chats.is_empty() {
            client
                .query(
                    r#"INSERT INTO chat.users (user_id, creation_date, name, chats) VALUES
                        (
                            ?,
                            toTimestamp(now()),
                            ?,
                            {}
                        )"#,
                    (user_id, user_name),
                )
                .await?;
        } else {
            client
                .query(
                    r#"INSERT INTO chat.users (user_id, creation_date, name, chats) VALUES
                        (
                            ?,
                            toTimestamp(now()),
                            ?,
                            ?
                        )"#,
                    (user_id, user_name, chats),
                )
                .await?;
        }
        Ok(())
    }

    async fn select_data_from_chats(client: &Session) -> Result<Vec<ChatsRow>, Box<dyn Error>> {
        let rows: Result<Vec<_>, _> = client
            .query(
                r#"SELECT chat_id, creation_date, name, users, chat_type FROM chat.chats"#,
                &[],
            )
            .await?
            .rows_typed_or_empty::<ChatsRow>()
            .collect();
        Ok(rows?)
    }

    async fn clear_database(client: &Session) -> Result<(), Box<dyn Error>> {
        client.query("DROP KEYSPACE IF EXISTS chat", &[]).await?;
        Ok(())
    }

    async fn select_data_from_users(client: &Session) -> Result<Vec<UsersRow>, Box<dyn Error>> {
        let rows: Result<Vec<_>, _> = client
            .query(
                r#"SELECT user_id, creation_date, name, chats FROM chat.users"#,
                &[],
            )
            .await?
            .rows_typed_or_empty::<UsersRow>()
            .collect();
        Ok(rows?)
    }

    async fn select_messages_from_chat(
        client: &Session,
        chat_id: Uuid,
    ) -> Result<Vec<MessageRow>, Box<dyn Error>> {
        let i = chat_id.to_string().replace("-", "_");
        let q = format!(
            "SELECT message_id, user_id, date, message_text FROM chat.chat_{}",
            i
        );
        let rows: Result<Vec<_>, _> = client
            .query(q, &[])
            .await?
            .rows_typed_or_empty::<MessageRow>()
            .collect();
        Ok(rows?)
    }

    #[actix::test]
    #[serial]
    async fn test_init() {
        let docker = Cli::default();
        let image = GenericImage::new("scylladb/scylla", "5.1.0")
            .with_exposed_port(9042)
            .with_wait_for(WaitFor::message_on_stderr("initialization completed."));
        let node = docker.run(image);
        let port = node.get_host_port_ipv4(9042);
        let database = ScyllaDatabase::new("localhost".into(), port).await.unwrap();
        clear_database(&database.client).await.unwrap();
        database.init_db().await.unwrap();
        let is_chats_table_empty = select_data_from_chats(&database.client)
            .await
            .unwrap()
            .is_empty();
        assert_eq!(
            true, is_chats_table_empty,
            "Chats table is not empty on db startup"
        );
        let is_users_table_empty = select_data_from_users(&database.client)
            .await
            .unwrap()
            .is_empty();
        assert_eq!(
            true, is_users_table_empty,
            "Users table is not empty on db startup"
        );
        insert_data_into_chats(&database.client, "Test chat", vec![1, 2, 3], "Group")
            .await
            .unwrap();
        insert_data_into_users(&database.client, 1, "Test user", vec![Uuid::new_v4()])
            .await
            .unwrap();
        database.init_db().await.unwrap();
        let is_chats_table_empty = select_data_from_chats(&database.client)
            .await
            .unwrap()
            .is_empty();
        assert_eq!(
            false, is_chats_table_empty,
            "Chats table is empty on db startup"
        );
        let is_users_table_empty = select_data_from_users(&database.client)
            .await
            .unwrap()
            .is_empty();
        assert_eq!(
            false, is_users_table_empty,
            "Users table is empty on db startup"
        );
    }

    #[actix::test]
    #[serial]
    async fn test_init_empty() {
        let docker = Cli::default();
        let image = GenericImage::new("scylladb/scylla", "5.1.0")
            .with_exposed_port(9042)
            .with_wait_for(WaitFor::message_on_stderr("initialization completed."));
        let node = docker.run(image);
        let port = node.get_host_port_ipv4(9042);
        let database = ScyllaDatabase::new("localhost".into(), port).await.unwrap();
        clear_database(&database.client).await.unwrap();
        database.init_db_clear().await.unwrap();
        let is_chats_table_empty = select_data_from_chats(&database.client)
            .await
            .unwrap()
            .is_empty();
        assert_eq!(
            true, is_chats_table_empty,
            "Chats table is not empty on db startup"
        );
        let is_users_table_empty = select_data_from_users(&database.client)
            .await
            .unwrap()
            .is_empty();
        assert_eq!(
            true, is_users_table_empty,
            "Users table is not empty on db startup"
        );
        insert_data_into_chats(&database.client, "Test chat", vec![1, 2, 3], "Group")
            .await
            .unwrap();
        insert_data_into_users(&database.client, 1, "Test user", vec![Uuid::new_v4()])
            .await
            .unwrap();
        database.init_db_clear().await.unwrap();
        let is_chats_table_empty = select_data_from_chats(&database.client)
            .await
            .unwrap()
            .is_empty();
        assert_eq!(
            true, is_chats_table_empty,
            "Chats table is not empty on db startup"
        );
        let is_users_table_empty = select_data_from_users(&database.client)
            .await
            .unwrap()
            .is_empty();
        assert_eq!(
            true, is_users_table_empty,
            "Users table is not empty on db startup"
        );
    }

    #[actix::test]
    #[serial]
    async fn test_chat_creation() {
        let docker = Cli::default();
        let image = GenericImage::new("scylladb/scylla", "5.1.0")
            .with_exposed_port(9042)
            .with_wait_for(WaitFor::message_on_stderr("initialization completed."));
        let node = docker.run(image);
        let port = node.get_host_port_ipv4(9042);
        let database = ScyllaDatabase::new("localhost".into(), port).await.unwrap();
        // Очищаем базу
        clear_database(&database.client).await.unwrap();
        // Инициализируем заново
        database.init_db_clear().await.unwrap();

        // Вставляем данные о пользователях
        insert_data_into_users(&database.client, 1, "Test user".into(), vec![])
            .await
            .unwrap();

        insert_data_into_users(&database.client, 2, "Invited Test user".into(), vec![])
            .await
            .unwrap();

        insert_data_into_users(&database.client, 3, "Invited Test user 2".into(), vec![])
            .await
            .unwrap();

        // Создаем новый чат
        let new_chat_info = database
            .create_new_chat(1, vec![2], ChatType::Private, "Test chat".into())
            .await
            .unwrap();

        // Получаем его сообщения
        let messages = select_messages_from_chat(&database.client, new_chat_info.id)
            .await
            .unwrap();

        // Должны быть пустыми
        assert!(messages.is_empty());

        // Получаем данные о пользователях из базы
        let mut users = select_data_from_users(&database.client).await.unwrap();
        users.sort_by(|a, b| a.user_id.cmp(&b.user_id));
        let mut users = users.into_iter();

        let (user_1, user_2) = (users.next().unwrap(), users.next().unwrap());

        // Получаем данные о чатах из базы
        let chats = select_data_from_chats(&database.client).await.unwrap();

        // Выбираем созданный чат
        let chat = chats
            .into_iter()
            .find(|chat| chat.chat_id == new_chat_info.id)
            .unwrap();
        let chat_users = chat.users.unwrap();
        assert_eq!(user_1.chats.unwrap(), user_2.chats.unwrap());
        assert!(chat_users.contains(&1));
        assert!(chat_users.contains(&2));
        assert_eq!("Test chat", &chat.name);
        assert_eq!("private", &chat.chat_type);

        let new_chat_info = database
            .create_new_chat(2, vec![1, 3], ChatType::Group, "Test group chat".into())
            .await
            .unwrap();

        let messages = select_messages_from_chat(&database.client, new_chat_info.id)
            .await
            .unwrap();

        assert!(messages.is_empty());

        let mut users = select_data_from_users(&database.client).await.unwrap();
        users.sort_by(|a, b| a.user_id.cmp(&b.user_id));
        let mut users = users.into_iter();

        let (user_1, user_2, user_3) = (
            users.next().unwrap(),
            users.next().unwrap(),
            users.next().unwrap(),
        );

        let chats = select_data_from_chats(&database.client).await.unwrap();
        let chat = chats
            .into_iter()
            .find(|chat| chat.chat_id == new_chat_info.id)
            .unwrap();
        let chat_users = chat.users.unwrap();
        assert!(user_1.chats.unwrap().contains(&chat.chat_id));
        assert!(user_2.chats.unwrap().contains(&chat.chat_id));
        assert!(user_3.chats.unwrap().contains(&chat.chat_id));
        assert!(chat_users.contains(&user_1.user_id));
        assert!(chat_users.contains(&user_2.user_id));
        assert!(chat_users.contains(&user_3.user_id));
        assert_eq!("Test user", &user_1.name);
        assert_eq!("Invited Test user", &user_2.name);
        assert_eq!("Invited Test user 2", &user_3.name);
        assert_eq!("Test group chat", &chat.name);
        assert_eq!("group", &chat.chat_type);
    }

    #[actix::test]
    #[serial]
    async fn test_message_addition() {
        let docker = Cli::default();
        let image = GenericImage::new("scylladb/scylla", "5.1.0")
            .with_exposed_port(9042)
            .with_wait_for(WaitFor::message_on_stderr("initialization completed."));
        let node = docker.run(image);
        let port = node.get_host_port_ipv4(9042);
        let database = ScyllaDatabase::new("localhost".into(), port).await.unwrap();
        clear_database(&database.client).await.unwrap();
        database.init_db_clear().await.unwrap();

        insert_data_into_users(&database.client, 1, "Test user".into(), vec![])
            .await
            .unwrap();

        insert_data_into_users(&database.client, 2, "Invited Test user".into(), vec![])
            .await
            .unwrap();

        let chat_info = database
            .create_new_chat(1, vec![2], ChatType::Private, "Test chat".into())
            .await
            .unwrap();
        let messages = select_messages_from_chat(&database.client, chat_info.id)
            .await
            .unwrap();
        assert!(messages.is_empty());

        let new_message = ChatMessage {
            chat_id: chat_info.id,
            sender_id: 1,
            date: SerializableDuration {
                timestamp: Duration::seconds(10),
            },
            msg_text: "Hello".into(),
        };
        database.add_new_message_to_chat(new_message).await.unwrap();
        let messages = select_messages_from_chat(&database.client, chat_info.id)
            .await
            .unwrap();
        assert_eq!(1, messages.len());
        assert_eq!(1, messages[0].user_id);
        assert_eq!("Hello", &messages[0].message_text);
    }

    #[actix::test]
    #[serial]
    async fn test_user_addition_to_chat() {
        let docker = Cli::default();
        let image = GenericImage::new("scylladb/scylla", "5.1.0")
            .with_exposed_port(9042)
            .with_wait_for(WaitFor::message_on_stderr("initialization completed."));
        let node = docker.run(image);
        let port = node.get_host_port_ipv4(9042);
        let database = ScyllaDatabase::new("localhost".into(), port).await.unwrap();
        clear_database(&database.client).await.unwrap();
        database.init_db_clear().await.unwrap();

        insert_data_into_users(&database.client, 1, "Test user".into(), vec![])
            .await
            .unwrap();

        insert_data_into_users(&database.client, 2, "Invited Test user".into(), vec![])
            .await
            .unwrap();

        insert_data_into_users(&database.client, 3, "Invited Test user 2".into(), vec![])
            .await
            .unwrap();

        let new_chat_info = database
            .create_new_chat(1, vec![2], ChatType::Group, "Test chat".into())
            .await
            .unwrap();

        database
            .add_user_to_chat(1, 3, new_chat_info.id)
            .await
            .unwrap();

        let chats = select_data_from_chats(&database.client).await.unwrap();

        let chat = chats
            .into_iter()
            .find(|chat| chat.chat_id == new_chat_info.id)
            .unwrap();
        let chat_users = chat.users.unwrap();
        assert!(chat_users.contains(&3));
        assert!(chat_users.contains(&2));
        assert!(chat_users.contains(&1));

        let mut users = select_data_from_users(&database.client).await.unwrap();
        users.sort_by(|a, b| a.user_id.cmp(&b.user_id));
        let mut users = users.into_iter();

        let (user_1, user_2, user_3) = (
            users.next().unwrap(),
            users.next().unwrap(),
            users.next().unwrap(),
        );

        assert!(user_1.chats.unwrap().contains(&new_chat_info.id));
        assert!(user_2.chats.unwrap().contains(&new_chat_info.id));
        assert!(user_3.chats.unwrap().contains(&new_chat_info.id));
    }

    #[actix::test]
    #[serial]
    async fn test_quitting_chat() {
        let docker = Cli::default();
        let image = GenericImage::new("scylladb/scylla", "5.1.0")
            .with_exposed_port(9042)
            .with_wait_for(WaitFor::message_on_stderr("initialization completed."));
        let node = docker.run(image);
        let port = node.get_host_port_ipv4(9042);
        let database = ScyllaDatabase::new("localhost".into(), port).await.unwrap();
        clear_database(&database.client).await.unwrap();
        database.init_db_clear().await.unwrap();

        insert_data_into_users(&database.client, 1, "Test user".into(), vec![])
            .await
            .unwrap();

        insert_data_into_users(&database.client, 2, "Invited Test user".into(), vec![])
            .await
            .unwrap();

        let new_chat_info = database
            .create_new_chat(1, vec![2], ChatType::Group, "Test chat".into())
            .await
            .unwrap();

        database.exit_chat(1, new_chat_info.id).await.unwrap();

        let mut users = select_data_from_users(&database.client).await.unwrap();
        users.sort_by(|a, b| a.user_id.cmp(&b.user_id));
        let mut users = users.into_iter();

        let (user_1, user_2) = (users.next().unwrap(), users.next().unwrap());

        let chat = select_data_from_chats(&database.client)
            .await
            .unwrap()
            .into_iter()
            .find(|c| c.chat_id == new_chat_info.id)
            .unwrap();
        let chat_users = chat.users.unwrap();
        assert!(!chat_users.contains(&1));
        assert!(chat_users.contains(&2));
        assert!(user_1.chats.is_none());
        assert!(user_2.chats.unwrap().contains(&new_chat_info.id));

        database.exit_chat(2, new_chat_info.id).await.unwrap();
        let is_chat_present = select_data_from_chats(&database.client)
            .await
            .unwrap()
            .into_iter()
            .find(|c| c.chat_id == new_chat_info.id)
            .is_some();
        assert!(!is_chat_present);
        let is_chat_history_present = select_messages_from_chat(&database.client, new_chat_info.id)
            .await
            .is_ok();
        assert!(!is_chat_history_present);
    }

    #[actix::test]
    #[serial]
    async fn test_chat_deletion() {
        let docker = Cli::default();
        let image = GenericImage::new("scylladb/scylla", "5.1.0")
            .with_exposed_port(9042)
            .with_wait_for(WaitFor::message_on_stderr("initialization completed."));
        let node = docker.run(image);
        let port = node.get_host_port_ipv4(9042);
        let database = ScyllaDatabase::new("localhost".into(), port).await.unwrap();
        clear_database(&database.client).await.unwrap();
        database.init_db_clear().await.unwrap();

        insert_data_into_users(&database.client, 1, "Test user".into(), vec![])
            .await
            .unwrap();

        insert_data_into_users(&database.client, 2, "Invited Test user".into(), vec![])
            .await
            .unwrap();

        let new_chat_info = database
            .create_new_chat(1, vec![2], ChatType::Group, "Test chat".into())
            .await
            .unwrap();

        database.delete_chat(new_chat_info.id).await.unwrap();

        let is_chat_present = select_data_from_chats(&database.client)
            .await
            .unwrap()
            .into_iter()
            .find(|c| c.chat_id == new_chat_info.id)
            .is_some();
        assert!(!is_chat_present);
        let is_chat_history_present = select_messages_from_chat(&database.client, new_chat_info.id)
            .await
            .is_ok();
        assert!(!is_chat_history_present);
    }

    #[actix::test]
    #[serial]
    async fn test_getting_chat_info() {
        let docker = Cli::default();
        let image = GenericImage::new("scylladb/scylla", "5.1.0")
            .with_exposed_port(9042)
            .with_wait_for(WaitFor::message_on_stderr("initialization completed."));
        let node = docker.run(image);
        let port = node.get_host_port_ipv4(9042);
        let database = ScyllaDatabase::new("localhost".into(), port).await.unwrap();
        clear_database(&database.client).await.unwrap();
        database.init_db_clear().await.unwrap();

        insert_data_into_users(&database.client, 1, "Test user".into(), vec![])
            .await
            .unwrap();

        insert_data_into_users(&database.client, 2, "Invited Test user".into(), vec![])
            .await
            .unwrap();

        let new_chat_info = database
            .create_new_chat(1, vec![2], ChatType::Group, "Test chat".into())
            .await
            .unwrap();

        let chat_info = database.get_chat_info(1, new_chat_info.id).await.unwrap();
        assert_eq!(new_chat_info.id, chat_info.id);
        assert_eq!(new_chat_info.users, chat_info.users);
        assert_eq!(new_chat_info.name, chat_info.name);
        match chat_info.chat_type {
            ChatType::Group => {}
            _ => {
                panic!("Bruh")
            }
        }
    }

    #[actix::test]
    #[serial]
    async fn test_getting_user_info() {
        let docker = Cli::default();
        let image = GenericImage::new("scylladb/scylla", "5.1.0")
            .with_exposed_port(9042)
            .with_wait_for(WaitFor::message_on_stderr("initialization completed."));
        let node = docker.run(image);
        let port = node.get_host_port_ipv4(9042);
        let database = ScyllaDatabase::new("localhost".into(), port).await.unwrap();
        clear_database(&database.client).await.unwrap();
        database.init_db_clear().await.unwrap();

        insert_data_into_users(&database.client, 1, "Test user".into(), vec![])
            .await
            .unwrap();

        insert_data_into_users(&database.client, 2, "Invited Test user".into(), vec![])
            .await
            .unwrap();

        let user_info = database.get_user_info(1).await.unwrap();

        assert_eq!(user_info.id, 1);
        assert_eq!(user_info.name, "Test user");
        assert_eq!(user_info.chats, vec!());

        let new_chat_info = database
            .create_new_chat(1, vec![2], ChatType::Group, "Test chat".into())
            .await
            .unwrap();

        let user_info = database.get_user_info(1).await.unwrap();

        assert_eq!(user_info.id, 1);
        assert_eq!(user_info.name, "Test user");
        assert_eq!(user_info.chats, vec!(new_chat_info.id));
    }

    #[actix::test]
    #[serial]
    async fn test_new_user_creation() {
        let docker = Cli::default();
        let image = GenericImage::new("scylladb/scylla", "5.1.0")
            .with_exposed_port(9042)
            .with_wait_for(WaitFor::message_on_stderr("initialization completed."));
        let node = docker.run(image);
        let port = node.get_host_port_ipv4(9042);
        let database = ScyllaDatabase::new("localhost".into(), port).await.unwrap();
        clear_database(&database.client).await.unwrap();
        database.init_db_clear().await.unwrap();

        let user_info = database
            .create_new_user(1, "Test user".into())
            .await
            .unwrap();
        assert_eq!(user_info.id, 1);
        assert_eq!(user_info.name, "Test user");
        assert_eq!(user_info.chats, vec!());

        let user_info = database
            .create_new_user(1, "Test usar".into())
            .await
            .unwrap();
        assert_eq!(user_info.id, 1);
        assert_ne!(user_info.name, "Test usar");
        assert_eq!(user_info.chats, vec!());
    }

    #[actix::test]
    #[serial]
    async fn test_getting_user_chats() {
        let docker = Cli::default();
        let image = GenericImage::new("scylladb/scylla", "5.1.0")
            .with_exposed_port(9042)
            .with_wait_for(WaitFor::message_on_stderr("initialization completed."));
        let node = docker.run(image);
        let port = node.get_host_port_ipv4(9042);
        let database = ScyllaDatabase::new("localhost".into(), port).await.unwrap();
        clear_database(&database.client).await.unwrap();
        database.init_db_clear().await.unwrap();

        insert_data_into_users(&database.client, 1, "Test user".into(), vec![])
            .await
            .unwrap();

        insert_data_into_users(&database.client, 2, "Invited Test user".into(), vec![])
            .await
            .unwrap();

        let user_chats = database.get_user_chats(1).await.unwrap();
        assert!(user_chats.is_empty());

        let new_chat_info = database
            .create_new_chat(1, vec![2], ChatType::Group, "Test chat".into())
            .await
            .unwrap();

        let user_chats = database.get_user_chats(1).await.unwrap();
        assert_eq!(vec!(new_chat_info.id), user_chats);
    }

    #[actix::test]
    #[serial]
    async fn test_getting_user_list() {
        let docker = Cli::default();
        let image = GenericImage::new("scylladb/scylla", "5.1.0")
            .with_exposed_port(9042)
            .with_wait_for(WaitFor::message_on_stderr("initialization completed."));
        let node = docker.run(image);
        let port = node.get_host_port_ipv4(9042);
        let database = ScyllaDatabase::new("localhost".into(), port).await.unwrap();
        clear_database(&database.client).await.unwrap();
        database.init_db_clear().await.unwrap();

        let list = database.get_user_list().await.unwrap();
        assert!(list.is_empty());

        insert_data_into_users(&database.client, 1, "Test user".into(), vec![])
            .await
            .unwrap();

        insert_data_into_users(&database.client, 2, "Invited Test user".into(), vec![])
            .await
            .unwrap();

        let mut list = database.get_user_list().await.unwrap();
        list.sort();
        assert_eq!(vec!(1, 2), list);
    }

    #[actix::test]
    #[serial]
    async fn test_getting_chat_history() {
        let docker = Cli::default();
        let image = GenericImage::new("scylladb/scylla", "5.1.0")
            .with_exposed_port(9042)
            .with_wait_for(WaitFor::message_on_stderr("initialization completed."));
        let node = docker.run(image);
        let port = node.get_host_port_ipv4(9042);
        let database = ScyllaDatabase::new("localhost".into(), port).await.unwrap();
        clear_database(&database.client).await.unwrap();
        database.init_db_clear().await.unwrap();

        insert_data_into_users(&database.client, 1, "Test user".into(), vec![])
            .await
            .unwrap();

        insert_data_into_users(&database.client, 2, "Invited Test user".into(), vec![])
            .await
            .unwrap();

        let new_chat_info = database
            .create_new_chat(1, vec![2], ChatType::Group, "Test chat".into())
            .await
            .unwrap();

        let (messages, _index) = database
            .get_chat_history_paged(1, new_chat_info.id, 10, None)
            .await
            .unwrap();

        assert!(messages.is_empty());

        for i in 0..20 {
            database
                .add_new_message_to_chat(ChatMessage {
                    chat_id: new_chat_info.id,
                    sender_id: 1,
                    date: SerializableDuration {
                        timestamp: Duration::seconds(10),
                    },
                    msg_text: format!("{i}"),
                })
                .await
                .unwrap();
        }

        let (messages, index) = database
            .get_chat_history_paged(1, new_chat_info.id, 10, None)
            .await
            .unwrap();

        assert!(!messages.is_empty());
        assert_eq!("19", &messages.first().unwrap().msg_text);
        assert_eq!("10", &messages.last().unwrap().msg_text);

        let (messages, index) = database
            .get_chat_history_paged(1, new_chat_info.id, 10, Some(index))
            .await
            .unwrap();

        assert!(!messages.is_empty());
        assert_eq!("9", &messages.first().unwrap().msg_text);
        assert_eq!("0", &messages.last().unwrap().msg_text);

        let (messages, _index) = database
            .get_chat_history_paged(1, new_chat_info.id, 10, Some(index))
            .await
            .unwrap();

        assert!(messages.is_empty());
    }
}
