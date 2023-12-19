use std::{error::Error, sync::atomic::AtomicU32};

use sqlx::sqlite::SqlitePool;

use crate::contact_model::{Contact, ContactErrors, ContactId};

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

    pub fn pop_id(&self) -> u32 {
        self.next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    pub async fn all(&self) -> Result<Vec<Contact>, Box<dyn Error>> {
        let contacts: Vec<Contact> = sqlx::query_as("SELECT * FROM contact")
            .fetch_all(&self.pool)
            .await?;
        Ok(contacts)
    }

    pub async fn search(&self, q: &str) -> Result<Vec<Contact>, Box<dyn Error>> {
        let contacts: Vec<Contact> = sqlx::query_as(
            r#"
            SELECT * FROM contact 
            WHERE
                first LIKE ("%" || ? || "%") OR
                last LIKE ("%" || ? || "%")"#,
        )
        .bind(q)
        .bind(q)
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

        Self::execute_save(&self.pool, contact).await?;

        Ok(Ok(()))
    }

    pub async fn find(&self, id: u32) -> Result<Option<Contact>, Box<dyn Error>> {
        let contact: Option<Contact> = sqlx::query_as("SELECT * FROM contact WHERE id = ?")
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

    async fn execute_save<'a>(
        executor: impl sqlx::sqlite::SqliteExecutor<'a>,
        contact: &Contact,
    ) -> Result<(), Box<dyn Error>> {
        sqlx::query(
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
        .await?;

        Ok(())
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
