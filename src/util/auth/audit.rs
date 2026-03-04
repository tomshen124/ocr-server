
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tracing::{error, info, warn};

pub struct AccessLogger {
    access_records: Arc<Mutex<Vec<AccessRecord>>>,
    security_events: Arc<Mutex<Vec<SecurityEvent>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessRecord {
    pub timestamp: DateTime<Utc>,
    pub client_id: String,
    pub client_name: String,
    pub api_path: String,
    pub remote_addr: String,
    pub access_key: String,
    pub result: AccessResult,
    pub response_time_ms: Option<u64>,
    pub user_agent: Option<String>,
    pub request_size: Option<u64>,
    pub response_size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AccessResult {
    Success,
    AuthFailed(String),
    PermissionDenied(String),
    RateLimited(String),
    ServerError(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityEvent {
    pub timestamp: DateTime<Utc>,
    pub event_type: SecurityEventType,
    pub severity: SecuritySeverity,
    pub client_id: Option<String>,
    pub remote_addr: String,
    pub description: String,
    pub details: HashMap<String, String>,
    pub handled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SecurityEventType {
    AuthenticationFailure,
    PermissionDenied,
    RateLimitTriggered,
    SuspiciousActivity,
    ConfigurationChange,
    AnomalousAccessPattern,
    SignatureVerificationFailure,
    TimestampAnomaly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, PartialOrd)]
pub enum SecuritySeverity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

impl AccessLogger {
    pub fn new() -> Self {
        Self {
            access_records: Arc::new(Mutex::new(Vec::new())),
            security_events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn log_successful_access(
        &self,
        client_id: String,
        client_name: String,
        api_path: String,
        remote_addr: String,
        access_key: String,
        response_time_ms: Option<u64>,
    ) {
        let record = AccessRecord {
            timestamp: Utc::now(),
            client_id: client_id.clone(),
            client_name: client_name.clone(),
            api_path: api_path.clone(),
            remote_addr: remote_addr.clone(),
            access_key,
            result: AccessResult::Success,
            response_time_ms,
            user_agent: None,
            request_size: None,
            response_size: None,
        };

        self.add_access_record(record);

        info!(
            event = "access_log",
            client_id = client_id,
            client_name = client_name,
            api_path = api_path,
            remote_addr = remote_addr,
            response_time_ms = response_time_ms,
            "[ok] 成功访问记录"
        );
    }

    pub fn log_failed_access(
        &self,
        client_id: Option<String>,
        api_path: String,
        remote_addr: String,
        access_key: Option<String>,
        error_reason: String,
        result: AccessResult,
    ) {
        let record = AccessRecord {
            timestamp: Utc::now(),
            client_id: client_id.clone().unwrap_or_else(|| "unknown".to_string()),
            client_name: "unknown".to_string(),
            api_path: api_path.clone(),
            remote_addr: remote_addr.clone(),
            access_key: access_key.unwrap_or_else(|| "unknown".to_string()),
            result: result.clone(),
            response_time_ms: None,
            user_agent: None,
            request_size: None,
            response_size: None,
        };

        self.add_access_record(record);

        let event_type = match result {
            AccessResult::AuthFailed(_) => SecurityEventType::AuthenticationFailure,
            AccessResult::PermissionDenied(_) => SecurityEventType::PermissionDenied,
            AccessResult::RateLimited(_) => SecurityEventType::RateLimitTriggered,
            _ => SecurityEventType::SuspiciousActivity,
        };

        self.log_security_event(
            event_type,
            SecuritySeverity::Medium,
            client_id,
            remote_addr,
            error_reason,
            HashMap::new(),
        );
    }

    pub fn log_security_event(
        &self,
        event_type: SecurityEventType,
        severity: SecuritySeverity,
        client_id: Option<String>,
        remote_addr: String,
        description: String,
        details: HashMap<String, String>,
    ) {
        let event = SecurityEvent {
            timestamp: Utc::now(),
            event_type: event_type.clone(),
            severity: severity.clone(),
            client_id: client_id.clone(),
            remote_addr: remote_addr.clone(),
            description: description.clone(),
            details,
            handled: false,
        };

        self.add_security_event(event);

        let log_level = match severity {
            SecuritySeverity::Info => "info",
            SecuritySeverity::Low => "warn",
            SecuritySeverity::Medium => "warn",
            SecuritySeverity::High => "error",
            SecuritySeverity::Critical => "error",
        };

        match log_level {
            "info" => info!(
                event = "security_event",
                event_type = ?event_type,
                severity = ?severity,
                client_id = client_id,
                remote_addr = remote_addr,
                description = description,
                "[locked] 安全事件"
            ),
            "warn" => warn!(
                event = "security_event",
                event_type = ?event_type,
                severity = ?severity,
                client_id = client_id,
                remote_addr = remote_addr,
                description = description,
                "[warn] 安全事件"
            ),
            "error" => error!(
                event = "security_event",
                event_type = ?event_type,
                severity = ?severity,
                client_id = client_id,
                remote_addr = remote_addr,
                description = description,
                "[alert] 安全事件"
            ),
            _ => {}
        }
    }

    fn add_access_record(&self, record: AccessRecord) {
        let mut records = self.access_records.lock().unwrap();
        records.push(record);

        if records.len() > 10000 {
            records.drain(0..1000);
        }
    }

    fn add_security_event(&self, event: SecurityEvent) {
        let mut events = self.security_events.lock().unwrap();
        events.push(event);

        if events.len() > 5000 {
            events.drain(0..500);
        }
    }

    pub fn get_access_statistics(&self, time_range: TimeRange) -> AccessStatistics {
        let records = self.access_records.lock().unwrap();
        let now = Utc::now();
        let cutoff_time = match time_range {
            TimeRange::LastHour => now - chrono::Duration::hours(1),
            TimeRange::LastDay => now - chrono::Duration::days(1),
            TimeRange::LastWeek => now - chrono::Duration::weeks(1),
            TimeRange::LastMonth => now - chrono::Duration::days(30),
        };

        let filtered_records: Vec<_> = records
            .iter()
            .filter(|record| record.timestamp > cutoff_time)
            .collect();

        let total_requests = filtered_records.len();
        let successful_requests = filtered_records
            .iter()
            .filter(|record| matches!(record.result, AccessResult::Success))
            .count();
        let failed_requests = total_requests - successful_requests;

        let mut client_stats = HashMap::new();
        let mut api_stats = HashMap::new();

        for record in &filtered_records {
            *client_stats.entry(record.client_id.clone()).or_insert(0) += 1;
            *api_stats.entry(record.api_path.clone()).or_insert(0) += 1;
        }

        let avg_response_time = if successful_requests > 0 {
            let total_time: u64 = filtered_records
                .iter()
                .filter_map(|record| record.response_time_ms)
                .sum();
            Some(total_time / successful_requests as u64)
        } else {
            None
        };

        AccessStatistics {
            time_range,
            total_requests,
            successful_requests,
            failed_requests,
            unique_clients: client_stats.len(),
            top_clients: Self::get_top_entries(client_stats, 10),
            top_apis: Self::get_top_entries(api_stats, 10),
            avg_response_time_ms: avg_response_time,
        }
    }

    pub fn get_security_statistics(&self, time_range: TimeRange) -> SecurityStatistics {
        let events = self.security_events.lock().unwrap();
        let now = Utc::now();
        let cutoff_time = match time_range {
            TimeRange::LastHour => now - chrono::Duration::hours(1),
            TimeRange::LastDay => now - chrono::Duration::days(1),
            TimeRange::LastWeek => now - chrono::Duration::weeks(1),
            TimeRange::LastMonth => now - chrono::Duration::days(30),
        };

        let filtered_events: Vec<_> = events
            .iter()
            .filter(|event| event.timestamp > cutoff_time)
            .collect();

        let total_events = filtered_events.len();
        let critical_events = filtered_events
            .iter()
            .filter(|event| event.severity == SecuritySeverity::Critical)
            .count();
        let high_events = filtered_events
            .iter()
            .filter(|event| event.severity == SecuritySeverity::High)
            .count();
        let unhandled_events = filtered_events
            .iter()
            .filter(|event| !event.handled)
            .count();

        let mut event_type_stats = HashMap::new();
        for event in &filtered_events {
            *event_type_stats
                .entry(format!("{:?}", event.event_type))
                .or_insert(0) += 1;
        }

        SecurityStatistics {
            time_range,
            total_events,
            critical_events,
            high_events,
            unhandled_events,
            event_types: event_type_stats,
        }
    }

    pub fn get_client_access_history(&self, client_id: &str, limit: usize) -> Vec<AccessRecord> {
        let records = self.access_records.lock().unwrap();
        records
            .iter()
            .filter(|record| record.client_id == client_id)
            .rev()
            .take(limit)
            .cloned()
            .collect()
    }

    pub fn get_unhandled_security_events(&self) -> Vec<SecurityEvent> {
        let events = self.security_events.lock().unwrap();
        events
            .iter()
            .filter(|event| !event.handled)
            .cloned()
            .collect()
    }

    pub fn mark_security_event_handled(&self, event_index: usize) {
        let mut events = self.security_events.lock().unwrap();
        if let Some(event) = events.get_mut(event_index) {
            event.handled = true;
        }
    }

    fn get_top_entries(mut stats: HashMap<String, usize>, limit: usize) -> Vec<(String, usize)> {
        let mut entries: Vec<_> = stats.drain().collect();
        entries.sort_by(|a, b| b.1.cmp(&a.1));
        entries.into_iter().take(limit).collect()
    }
}

#[derive(Debug, Clone, Copy)]
pub enum TimeRange {
    LastHour,
    LastDay,
    LastWeek,
    LastMonth,
}

#[derive(Debug, Clone)]
pub struct AccessStatistics {
    pub time_range: TimeRange,
    pub total_requests: usize,
    pub successful_requests: usize,
    pub failed_requests: usize,
    pub unique_clients: usize,
    pub top_clients: Vec<(String, usize)>,
    pub top_apis: Vec<(String, usize)>,
    pub avg_response_time_ms: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct SecurityStatistics {
    pub time_range: TimeRange,
    pub total_events: usize,
    pub critical_events: usize,
    pub high_events: usize,
    pub unhandled_events: usize,
    pub event_types: HashMap<String, usize>,
}

impl Default for AccessLogger {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_access_logger_basic() {
        let logger = AccessLogger::new();

        logger.log_successful_access(
            "test_client".to_string(),
            "Test Client".to_string(),
            "/api/test".to_string(),
            "127.0.0.1".to_string(),
            "test_key".to_string(),
            Some(100),
        );

        let stats = logger.get_access_statistics(TimeRange::LastHour);
        assert_eq!(stats.total_requests, 1);
        assert_eq!(stats.successful_requests, 1);
        assert_eq!(stats.failed_requests, 0);
    }

    #[test]
    fn test_security_event_logging() {
        let logger = AccessLogger::new();

        logger.log_security_event(
            SecurityEventType::AuthenticationFailure,
            SecuritySeverity::High,
            Some("test_client".to_string()),
            "127.0.0.1".to_string(),
            "Invalid credentials".to_string(),
            HashMap::new(),
        );

        let unhandled_events = logger.get_unhandled_security_events();
        assert_eq!(unhandled_events.len(), 1);
        assert_eq!(unhandled_events[0].severity, SecuritySeverity::High);
    }

    #[test]
    fn test_access_statistics() {
        let logger = AccessLogger::new();

        for i in 0..5 {
            logger.log_successful_access(
                format!("client_{}", i % 2),
                format!("Client {}", i % 2),
                "/api/test".to_string(),
                "127.0.0.1".to_string(),
                "test_key".to_string(),
                Some(100 + i * 10),
            );
        }

        let stats = logger.get_access_statistics(TimeRange::LastHour);
        assert_eq!(stats.total_requests, 5);
        assert_eq!(stats.successful_requests, 5);
        assert_eq!(stats.unique_clients, 2);
    }
}
