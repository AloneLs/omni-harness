use chrono::{DateTime, Datelike, Timelike, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledJob {
    pub id: String,
    pub name: String,
    pub cron_expression: String,
    pub task_prompt: String,
    pub is_active: bool,
    pub last_run_at: Option<String>,
    pub next_run_at: Option<String>,
    pub run_count: u64,
}

#[derive(Debug, Clone)]
pub struct CronField {
    pub allowed_values: HashSet<u32>,
}

impl CronField {
    pub fn parse(field_str: &str, min: u32, max: u32) -> Result<Self, String> {
        let mut values = HashSet::new();

        for part in field_str.split(',') {
            let part = part.trim();
            if part == "*" {
                for v in min..=max {
                    values.insert(v);
                }
            } else if part.starts_with('*') && part.chars().nth(1) == Some('/') {
                let step_str = &part[2..];
                let step: u32 = step_str
                    .parse()
                    .map_err(|e| format!("Invalid cron step '{}': {}", step_str, e))?;
                if step == 0 {
                    return Err("Cron step cannot be 0".to_string());
                }
                let mut v = min;
                while v <= max {
                    values.insert(v);
                    v += step;
                }
            } else if part.contains('-') {
                let range_parts: Vec<&str> = part.split('-').collect();
                if range_parts.len() != 2 {
                    return Err(format!("Invalid range in cron: {}", part));
                }
                let start: u32 = range_parts[0]
                    .parse()
                    .map_err(|e| format!("Invalid range start: {}", e))?;
                let end: u32 = range_parts[1]
                    .parse()
                    .map_err(|e| format!("Invalid range end: {}", e))?;
                if start > end || start < min || end > max {
                    return Err(format!("Range out of bounds: {}-{}", start, end));
                }
                for v in start..=end {
                    values.insert(v);
                }
            } else {
                let v: u32 = part
                    .parse()
                    .map_err(|e| format!("Invalid number in cron '{}': {}", part, e))?;
                if v < min || v > max {
                    return Err(format!("Value {} out of bounds [{}-{}]", v, min, max));
                }
                values.insert(v);
            }
        }

        Ok(Self {
            allowed_values: values,
        })
    }

    pub fn contains(&self, val: u32) -> bool {
        self.allowed_values.contains(&val)
    }
}

#[derive(Debug, Clone)]
pub struct CronSchedule {
    pub minutes: CronField,
    pub hours: CronField,
    pub days_of_month: CronField,
    pub months: CronField,
    pub days_of_week: CronField,
}

impl CronSchedule {
    pub fn parse(expr: &str) -> Result<Self, String> {
        let parts: Vec<&str> = expr.split_whitespace().collect();
        if parts.len() != 5 {
            return Err(format!(
                "Cron expression must contain exactly 5 fields, found {}",
                parts.len()
            ));
        }

        let minutes = CronField::parse(parts[0], 0, 59)?;
        let hours = CronField::parse(parts[1], 0, 23)?;
        let days_of_month = CronField::parse(parts[2], 1, 31)?;
        let months = CronField::parse(parts[3], 1, 12)?;
        let days_of_week = CronField::parse(parts[4], 0, 6)?;

        Ok(Self {
            minutes,
            hours,
            days_of_month,
            months,
            days_of_week,
        })
    }

    pub fn matches(&self, dt: &DateTime<Utc>) -> bool {
        self.minutes.contains(dt.minute())
            && self.hours.contains(dt.hour())
            && self.days_of_month.contains(dt.day())
            && self.months.contains(dt.month())
            && self.days_of_week.contains(dt.weekday().num_days_from_sunday())
    }

    pub fn next_run_after(&self, from: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let mut candidate = from + chrono::Duration::minutes(1);
        candidate = candidate
            .with_second(0)?
            .with_nanosecond(0)?;

        let max_iterations = 60 * 24 * 366;
        for _ in 0..max_iterations {
            if self.matches(&candidate) {
                return Some(candidate);
            }
            candidate = candidate + chrono::Duration::minutes(1);
        }
        None
    }
}

pub struct CronScheduler {
    jobs: Arc<RwLock<HashMap<String, ScheduledJob>>>,
    running_jobs: Arc<RwLock<HashSet<String>>>,
    is_active: Arc<AtomicBool>,
}

impl Default for CronScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl CronScheduler {
    pub fn new() -> Self {
        Self {
            jobs: Arc::new(RwLock::new(HashMap::new())),
            running_jobs: Arc::new(RwLock::new(HashSet::new())),
            is_active: Arc::new(AtomicBool::new(false)),
        }
    }

    pub async fn add_job(&self, mut job: ScheduledJob) -> Result<(), String> {
        let schedule = CronSchedule::parse(&job.cron_expression)?;
        let next = schedule.next_run_after(Utc::now());
        job.next_run_at = next.map(|dt| dt.to_rfc3339());

        let mut map = self.jobs.write().await;
        map.insert(job.id.clone(), job);
        Ok(())
    }

    pub async fn update_job(&self, mut job: ScheduledJob) -> Result<(), String> {
        let schedule = CronSchedule::parse(&job.cron_expression)?;
        let next = schedule.next_run_after(Utc::now());
        job.next_run_at = next.map(|dt| dt.to_rfc3339());

        let mut map = self.jobs.write().await;
        if map.contains_key(&job.id) {
            map.insert(job.id.clone(), job);
            Ok(())
        } else {
            Err(format!("Job '{}' not found", job.id))
        }
    }

    pub async fn delete_job(&self, id: &str) -> bool {
        let mut map = self.jobs.write().await;
        map.remove(id).is_some()
    }

    pub async fn list_jobs(&self) -> Vec<ScheduledJob> {
        let map = self.jobs.read().await;
        map.values().cloned().collect()
    }

    pub async fn get_job(&self, id: &str) -> Option<ScheduledJob> {
        let map = self.jobs.read().await;
        map.get(id).cloned()
    }

    pub async fn has_running_jobs(&self) -> bool {
        let running = self.running_jobs.read().await;
        !running.is_empty()
    }

    pub fn start<F>(&self, executor: F)
    where
        F: Fn(ScheduledJob) + Send + Sync + 'static,
    {
        self.is_active.store(true, Ordering::SeqCst);
        let is_running = self.is_active.clone();
        let jobs_ref = self.jobs.clone();
        let running_jobs_ref = self.running_jobs.clone();
        let exec_arc = Arc::new(executor);

        tokio::spawn(async move {
            while is_running.load(Ordering::SeqCst) {
                let now = Utc::now();
                let mut jobs_to_run = Vec::new();

                {
                    let map = jobs_ref.read().await;
                    let running = running_jobs_ref.read().await;

                    for job in map.values() {
                        if !job.is_active {
                            continue;
                        }

                        if running.contains(&job.id) {
                            continue;
                        }

                        let should_run = if let Some(ref next_str) = job.next_run_at {
                            if let Ok(next_dt) = DateTime::parse_from_rfc3339(next_str) {
                                now >= next_dt.with_timezone(&Utc)
                            } else {
                                false
                            }
                        } else if let Ok(parsed) = CronSchedule::parse(&job.cron_expression) {
                            parsed.matches(&now)
                        } else {
                            false
                        };

                        if should_run {
                            jobs_to_run.push(job.clone());
                        }
                    }
                }

                for job in jobs_to_run {
                    {
                        let mut running = running_jobs_ref.write().await;
                        running.insert(job.id.clone());
                    }

                    {
                        let mut map = jobs_ref.write().await;
                        if let Some(j) = map.get_mut(&job.id) {
                            j.last_run_at = Some(now.to_rfc3339());
                            j.run_count += 1;
                            if let Ok(sched) = CronSchedule::parse(&j.cron_expression) {
                                j.next_run_at = sched.next_run_after(now).map(|d| d.to_rfc3339());
                            }
                        }
                    }

                    let running_jobs_clone = running_jobs_ref.clone();
                    let exec_clone = exec_arc.clone();
                    let job_clone = job.clone();

                    tokio::spawn(async move {
                        (exec_clone)(job_clone.clone());
                        let mut running = running_jobs_clone.write().await;
                        running.remove(&job_clone.id);
                    });
                }

                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });
    }

    pub fn stop(&self) {
        self.is_active.store(false, Ordering::SeqCst);
    }
}
