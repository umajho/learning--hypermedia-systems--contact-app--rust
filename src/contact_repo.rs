use std::{error::Error, sync::atomic::AtomicU32};

use sqlx::sqlite::SqlitePool;

use crate::contact_model::{Contact, ContactErrors, ContactId};

const ERR_EMAIL_UNIQUE: &str = "Email Must Be Unique";

/// TODO: move to somewhere more properly.
pub const PAGE_SIZE: u32 = 10;

pub struct ContactRepo {
    pool: SqlitePool,

    next_id: AtomicU32,
}
impl ContactRepo {
    pub async fn build(pool: SqlitePool) -> Result<Self, Box<dyn Error>> {
        sqlx::query(
            "
            CREATE TABLE contact (
                id      INTEGER PRIMARY KEY,
                first   TEXT,
                last    TEXT,
                phone   TEXT,
                email   TEXT UNIQUE NOT NULL
            )
        ",
        )
        .execute(&pool)
        .await?;

        Ok(Self {
            pool,

            next_id: AtomicU32::new(0),
        })
    }
    pub async fn build_with_fake_data(pool: SqlitePool, n: u32) -> Result<Self, Box<dyn Error>> {
        let c = Self::build(pool).await?;

        {
            let mut tx = c.pool.begin().await?;

            for id in 0..n {
                let contact = Contact::new_fake(ContactId::new(id));
                Self::execute_save(&mut *tx, &contact).await?;
            }
            c.next_id.store(n, std::sync::atomic::Ordering::Relaxed);

            tx.commit().await?;
        }

        Ok(c)
    }

    pub fn pop_id(&self) -> ContactId {
        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        ContactId::new(id)
    }

    pub async fn count(&self) -> Result<u32, Box<dyn Error>> {
        let (count,): (u32,) = sqlx::query_as("SELECT count(*) FROM contact")
            .fetch_one(&self.pool)
            .await?;

        Ok(count)
    }

    pub async fn all(&self) -> Result<Vec<Contact>, Box<dyn Error>> {
        let contacts: Vec<Contact> = sqlx::query_as("SELECT * FROM contact")
            .fetch_all(&self.pool)
            .await?;
        Ok(contacts)
    }

    pub async fn all_by_page(&self, page: u32) -> Result<Vec<Contact>, Box<dyn Error>> {
        let page = page.max(1);

        let contacts: Vec<Contact> = sqlx::query_as(
            r#"SELECT * FROM contact
            LIMIT ? OFFSET ?"#,
        )
        .bind(PAGE_SIZE)
        .bind((page - 1) * PAGE_SIZE)
        .fetch_all(&self.pool)
        .await?;
        Ok(contacts)
    }

    pub async fn search(&self, q: &str, page: u32) -> Result<Vec<Contact>, Box<dyn Error>> {
        let page = page.max(1);

        let contacts: Vec<Contact> = sqlx::query_as(
            r#"
            SELECT * FROM contact 
            WHERE
                first LIKE ("%" || ? || "%") OR
                last LIKE ("%" || ? || "%")
                LIMIT ? OFFSET ?"#,
        )
        .bind(q)
        .bind(q)
        .bind(PAGE_SIZE)
        .bind((page - 1) * PAGE_SIZE)
        .fetch_all(&self.pool)
        .await?;
        Ok(contacts)
    }

    pub async fn save(
        &self,
        contact: &Contact,
    ) -> Result<Result<(), ContactErrors>, Box<dyn Error>> {
        if let Err(errors) = contact.validate() {
            return Ok(Err(errors));
        }

        if !Self::execute_save(&self.pool, contact).await? {
            return Ok(Err(ContactErrors {
                email: Some(ERR_EMAIL_UNIQUE.to_string()),
                ..Default::default()
            }));
        };

        Ok(Ok(()))
    }

    pub async fn find(&self, id: ContactId) -> Result<Option<Contact>, Box<dyn Error>> {
        let contact: Option<Contact> = sqlx::query_as("SELECT * FROM contact WHERE id = ?")
            .bind(id.value())
            .fetch_optional(&self.pool)
            .await?;

        Ok(contact)
    }

    pub async fn find_by_email(&self, id: String) -> Result<Option<Contact>, Box<dyn Error>> {
        let contact: Option<Contact> = sqlx::query_as("SELECT * FROM contact WHERE email = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;

        Ok(contact)
    }

    pub async fn update(
        &self,
        contact: &Contact,
    ) -> Result<Result<(), ContactErrors>, Box<dyn Error>> {
        if let Err(errors) = contact.validate() {
            return Ok(Err(errors));
        }

        Self::execute_update(&self.pool, contact).await?;

        Ok(Ok(()))
    }

    pub async fn delete(&self, contact_id: ContactId) -> Result<(), Box<dyn Error>> {
        Self::execute_delete(&self.pool, contact_id).await?;

        Ok(())
    }

    pub async fn validate_email(
        &self,
        contact_id: Option<ContactId>,
        email: String,
    ) -> Result<Option<String>, Box<dyn Error>> {
        if let Some(err) = Contact::validate_email(&email) {
            return Ok(Some(err));
        }

        let Some(contact_with_email) = self.find_by_email(email).await? else {
            return Ok(None);
        };

        match contact_id {
            Some(contact_id) if contact_id == contact_with_email.id() => Ok(None),
            _ => Ok(Some(ERR_EMAIL_UNIQUE.to_string())),
        }
    }

    async fn execute_save<'a>(
        executor: impl sqlx::sqlite::SqliteExecutor<'a>,
        contact: &Contact,
    ) -> Result<bool, Box<dyn Error>> {
        let result = sqlx::query(
            "
            INSERT INTO contact (id, first, last, phone, email)
            VALUES (?, ?, ?, ?, ?)
        ",
        )
        .bind(contact.id().value())
        .bind(contact.first())
        .bind(contact.last())
        .bind(contact.phone())
        .bind(contact.email())
        .execute(executor)
        .await;
        match result {
            Ok(_) => Ok(true),
            Err(err) => 'err: {
                if let Some(err) = err.as_database_error() {
                    if err.is_unique_violation() {
                        break 'err Ok(false);
                    }
                }
                Err(err.into())
            }
        }
    }

    async fn execute_update<'a>(
        executor: impl sqlx::sqlite::SqliteExecutor<'a>,
        contact: &Contact,
    ) -> Result<(), Box<dyn Error>> {
        sqlx::query(
            "
            UPDATE contact
            SET first = ?, last = ?, phone = ?, email = ?
            WHERE id = ?
        ",
        )
        .bind(contact.first())
        .bind(contact.last())
        .bind(contact.phone())
        .bind(contact.email())
        .bind(contact.id().value())
        .execute(executor)
        .await?;

        Ok(())
    }

    async fn execute_delete<'a>(
        executor: impl sqlx::sqlite::SqliteExecutor<'a>,
        contact_id: ContactId,
    ) -> Result<(), Box<dyn Error>> {
        sqlx::query("DELETE FROM contact WHERE id = ?")
            .bind(contact_id.value())
            .execute(executor)
            .await?;

        Ok(())
    }
}
