use std::{
    sync::{atomic::AtomicU8, Arc},
    time::Duration,
};

use arc_swap::ArcSwapOption;

use crate::contact_repo::ContactRepo;

#[atomic_enum::atomic_enum]
#[derive(PartialEq)]
pub enum Status {
    Waiting,
    Running,
    Complete,
}

pub struct Archiver {
    contacts: Arc<ContactRepo>,

    status: AtomicStatus,
    progress_percentage: AtomicU8,
    json_data: ArcSwapOption<String>,
}

impl Archiver {
    pub fn new(contacts: Arc<ContactRepo>) -> Self {
        Self {
            contacts,
            status: AtomicStatus::new(Status::Waiting),
            progress_percentage: AtomicU8::new(0),
            json_data: ArcSwapOption::from(None),
        }
    }

    pub fn status(&self) -> Status {
        self.status.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn progress(&self) -> f32 {
        self.progress_percentage
            .load(std::sync::atomic::Ordering::Relaxed) as f32
            / 100.0
    }

    pub fn json_data(&self) -> Option<Arc<String>> {
        self.json_data.load_full()
    }

    pub fn run(self: &Arc<Self>) {
        let old_status = self
            .status
            .swap(Status::Running, std::sync::atomic::Ordering::Relaxed);
        if old_status != Status::Waiting {
            if old_status == Status::Complete {
                self.status
                    .store(Status::Complete, std::sync::atomic::Ordering::Relaxed);
            }
            return;
        }
        self.progress_percentage
            .store(0, std::sync::atomic::Ordering::Relaxed);

        let archiver = self.clone();
        tokio::spawn(async move {
            for i in 0..10 {
                tokio::time::sleep(Duration::from_secs_f64(rand::random())).await;
                if archiver.status() != Status::Running {
                    return;
                }
                archiver
                    .progress_percentage
                    .store((i + 1) * 10, std::sync::atomic::Ordering::Relaxed);
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
            if archiver.status() != Status::Running {
                return;
            }
            archiver.json_data.store(Some(Arc::new({
                let contacts = archiver.contacts.all().await.unwrap();
                serde_json::to_string(&contacts).unwrap()
            })));
            archiver
                .status
                .store(Status::Complete, std::sync::atomic::Ordering::Relaxed)
        });
    }

    pub fn reset(&self) {
        self.status
            .store(Status::Waiting, std::sync::atomic::Ordering::Relaxed);
    }
}
