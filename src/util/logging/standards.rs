//! Centralised logging metadata (event names, shared keys, etc.).

/// Canonical event names used across the service.
pub mod events {
    /// HTTP request lifecycle.
    pub const REQUEST_START: &str = "request.start";
    pub const REQUEST_COMPLETE: &str = "request.complete";
    pub const REQUEST_ERROR: &str = "request.error";
    pub const REQUEST_SLOW: &str = "request.slow";
    pub const REQUEST_METRICS: &str = "request.metrics";

    /// Authentication flow.
    pub const AUTH_CHECK: &str = "auth.check";
    pub const AUTH_SUCCESS: &str = "auth.success";
    pub const AUTH_FAILURE: &str = "auth.failure";
    pub const AUTH_ERROR: &str = "auth.error";

    /// Preview pipeline lifecycle.
    pub const PREVIEW_RECEIVED: &str = "preview.received";
    pub const PREVIEW_DISPATCH: &str = "preview.dispatch";
    pub const PREVIEW_VALIDATE_FAILED: &str = "preview.validate_failed";
    pub const PREVIEW_COMPLETE: &str = "preview.complete";
    pub const PREVIEW_ERROR: &str = "preview.error";
    pub const PREVIEW_SLOW_STEP: &str = "preview.slow_step";
    pub const PREVIEW_AUTO_LOGIN: &str = "preview.auto_login";
    pub const PREVIEW_AUTO_LOGIN_FAILED: &str = "preview.auto_login_failed";

    /// Generic API call tracking.
    pub const API_CALL_RECORDED: &str = "api.call_recorded";

    /// Material / attachment processing.
    pub const MATERIAL_BATCH_START: &str = "material.batch_start";
    pub const MATERIAL_BATCH_COMPLETE: &str = "material.batch_complete";
    pub const MATERIAL_STATS: &str = "material.batch_stats";
    pub const MATERIAL_START: &str = "material.start";
    pub const MATERIAL_COMPLETE: &str = "material.complete";
    pub const MATERIAL_ERROR: &str = "material.error";

    pub const ATTACHMENT_START: &str = "attachment.start";
    pub const ATTACHMENT_COMPLETE: &str = "attachment.complete";
    pub const ATTACHMENT_REUSED: &str = "attachment.reused";
    pub const ATTACHMENT_SLOW: &str = "attachment.slow";
    pub const ATTACHMENT_ERROR: &str = "attachment.error";
    pub const ATTACHMENT_DOWNLOAD_START: &str = "attachment.download_start";
    pub const ATTACHMENT_DOWNLOAD_COMPLETE: &str = "attachment.download_complete";
    pub const ATTACHMENT_OCR_COMPLETE: &str = "attachment.ocr_complete";
    pub const ATTACHMENT_UPLOAD_PROFILE: &str = "attachment.upload_profile";
    pub const ATTACHMENT_PROFILE: &str = "attachment.profile";

    /// 队列与 Worker。
    pub const QUEUE_ENQUEUE: &str = "queue.enqueue";
    pub const QUEUE_DEQUEUE: &str = "queue.dequeue";
    pub const WORKER_FETCH_MATERIAL: &str = "worker.fetch_material";
    pub const WORKER_FETCH_FAILURE: &str = "worker.fetch_failure";
    pub const WORKER_RESULT_SUBMIT: &str = "worker.submit_result";

    /// Processing pipeline / resource controller.
    pub const PIPELINE_START: &str = "processing.pipeline_start";
    pub const PIPELINE_STAGE: &str = "processing.stage";
    pub const PIPELINE_SLOW: &str = "processing.slow_stage";
    pub const PIPELINE_COMPLETE: &str = "processing.complete";
    pub const PIPELINE_ERROR: &str = "processing.error";
}
