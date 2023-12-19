use sqlx::FromRow;
use typed_builder::TypedBuilder;

#[derive(Default)]
pub struct ContactErrors {
    pub first: Option<String>,
    pub last: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
}

#[derive(Clone, Copy)]
pub struct ContactId(u32);
impl ContactId {
    pub fn new(id: u32) -> Self {
        Self(id)
    }

    pub fn value(&self) -> u32 {
        self.0
    }
}

#[derive(Clone, TypedBuilder)]
pub struct Contact {
    id: ContactId,
    first: String,
    last: String,
    phone: String,
    email: String,
}
impl Contact {
    pub fn new_fake(id: ContactId) -> Self {
        Self {
            id,
            first: fakeit::name::first(),
            last: fakeit::name::last(),
            phone: fakeit::contact::phone(),
            email: fakeit::contact::email(),
        }
    }

    pub fn match_text(&self, str: &str) -> bool {
        let str = str.to_lowercase();

        for item in [&self.first, &self.last] {
            if item.to_lowercase().contains(&str) {
                return true;
            }
        }
        false
    }

    pub fn validate(&self) -> Result<(), ContactErrors> {
        let err_email = if self.email.is_empty() {
            Some("Email Required".to_string())
        } else if !validator::validate_email(&self.email) {
            Some("Email Not Valid".to_string())
        } else {
            None
        };

        let err_phone = /*if !self.phone.is_empty() && !validator::validate_phone(&self.phone) {
            Some("Phone Not Valid".to_string())
        } else {
            None
        };*/ None;

        if err_email.is_some() || err_phone.is_some() {
            Err(ContactErrors {
                phone: err_phone,
                email: err_email,
                ..Default::default()
            })
        } else {
            Ok(())
        }
    }

    pub fn id(&self) -> ContactId {
        self.id
    }
    pub fn first(&self) -> &str {
        &self.first
    }
    pub fn last(&self) -> &str {
        &self.last
    }
    pub fn phone(&self) -> &str {
        &self.phone
    }
    pub fn email(&self) -> &str {
        &self.email
    }
}
impl<'r, R: sqlx::Row> FromRow<'r, R> for Contact
where
    &'r str: sqlx::ColumnIndex<R>,
    u32: sqlx::Type<R::Database>,
    u32: sqlx::Decode<'r, R::Database>,
    String: sqlx::Type<R::Database>,
    String: sqlx::Decode<'r, R::Database>,
{
    /// See: <https://stackoverflow.com/a/66713961>.
    fn from_row(row: &'r R) -> sqlx::Result<Self> {
        Ok(Self {
            id: ContactId::new(row.try_get("id")?),
            first: row.try_get("first")?,
            last: row.try_get("last")?,
            phone: row.try_get("phone")?,
            email: row.try_get("email")?,
        })
    }
}
