//! 智能资源预测模块
//! 根据文件特征预测资源需求，实现精准的资源分配

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use sysinfo::System;
use tracing::{debug, warn};

use crate::util::logging::standards::events;
use crate::util::tracing::metrics_collector::METRICS_COLLECTOR;

/// 任务资源预测器
pub struct TaskResourcePredictor;

/// 任务资源需求档案
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResourceProfile {
    pub file_size_mb: f64,
    pub file_type: String,
    pub estimated_pages: u32,
    pub predicted_stages: Vec<StageResourceNeed>,
    pub total_estimated_duration_seconds: u32,
    pub peak_memory_mb: u32,
    pub risk_level: RiskLevel,
    pub execution_recommendation: ExecutionRecommendation,
}

/// 单个阶段资源需求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageResourceNeed {
    pub stage: ProcessingStage,
    pub memory_mb: u32,
    pub duration_seconds: u32,
    pub cpu_intensive: bool,
    pub io_intensive: bool,
    pub concurrency_recommendation: u32, // 建议并发数
}

/// 处理阶段枚举
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ProcessingStage {
    Download,
    PdfConvert,
    OcrProcess,
    Storage,
}

/// 风险级别
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RiskLevel {
    Low,      // < 2GB 峰值内存
    Medium,   // 2-4GB 峰值内存
    High,     // 4-6GB 峰值内存
    Critical, // > 6GB 峰值内存
}

/// 执行建议
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExecutionRecommendation {
    Execute,            // 安全执行
    ExecuteWithCaution, // 谨慎执行，需要监控
    Defer,              // 延迟执行，等待资源释放
    Split,              // 建议分割处理
    Reject,             // 拒绝执行，文件过大
}

/// 系统执行能力评估
#[derive(Debug, Serialize)]
pub struct TaskExecutability {
    pub can_execute: bool,
    pub available_memory_gb: u32,
    pub required_memory_gb: u32,
    pub safety_margin_gb: u32,
    pub recommendation: ExecutionRecommendation,
    pub estimated_wait_time_seconds: u32,
    pub bottleneck_stage: Option<ProcessingStage>,
    pub optimization_suggestions: Vec<String>,
}

impl TaskResourcePredictor {
    /// 预测任务资源需求
    pub fn predict_task_resources(file_size_bytes: usize, file_type: &str) -> TaskResourceProfile {
        let file_size_mb = file_size_bytes as f64 / (1024.0 * 1024.0);

        debug!(
            target: "processing.pipeline",
            event = events::PIPELINE_STAGE,
            stage = "resource_predict_start",
            file_type = %file_type,
            file_size_mb
        );

        let estimated_pages = Self::estimate_pages(file_size_mb, file_type);
        let predicted_stages =
            Self::predict_processing_stages(file_size_mb, file_type, estimated_pages);
        let peak_memory_mb = predicted_stages
            .iter()
            .map(|s| s.memory_mb)
            .max()
            .unwrap_or(500);
        let total_duration = predicted_stages.iter().map(|s| s.duration_seconds).sum();

        let risk_level = Self::assess_risk_level(peak_memory_mb, estimated_pages, file_type);
        let execution_recommendation =
            Self::generate_execution_recommendation(peak_memory_mb, &risk_level, estimated_pages);

        debug!(
            target: "processing.pipeline",
            event = events::PIPELINE_STAGE,
            stage = "resource_predict_complete",
            peak_memory_mb,
            estimated_pages,
            risk = ?risk_level
        );

        let mut labels = HashMap::new();
        labels.insert("file_type".to_string(), file_type.to_string());
        labels.insert("risk".to_string(), format!("{:?}", risk_level));
        METRICS_COLLECTOR.record_pipeline_stage(
            "resource_predict",
            true,
            Duration::from_millis(0),
            Some(labels),
            None,
        );

        TaskResourceProfile {
            file_size_mb,
            file_type: file_type.to_string(),
            estimated_pages,
            predicted_stages,
            total_estimated_duration_seconds: total_duration,
            peak_memory_mb,
            risk_level,
            execution_recommendation,
        }
    }

    /// 估算文件页数
    fn estimate_pages(file_size_mb: f64, file_type: &str) -> u32 {
        match file_type.to_uppercase().as_str() {
            "PDF" => {
                // PDF页数估算模型：基于文件大小和内容复杂度
                // 简单PDF: ~50KB/页, 复杂PDF: ~200KB/页, 平均100KB/页
                let estimated_pages = (file_size_mb * 1024.0 / 100.0).ceil() as u32;
                // 限制在合理范围内
                estimated_pages.max(1).min(1000)
            }
            "JPG" | "JPEG" | "PNG" | "BMP" | "TIFF" => 1,
            _ => 1,
        }
    }

    /// 预测各处理阶段资源需求
    fn predict_processing_stages(
        file_size_mb: f64,
        file_type: &str,
        pages: u32,
    ) -> Vec<StageResourceNeed> {
        match file_type.to_uppercase().as_str() {
            "PDF" => Self::predict_pdf_stages(file_size_mb, pages),
            "JPG" | "JPEG" | "PNG" | "BMP" | "TIFF" => Self::predict_image_stages(file_size_mb),
            _ => Self::predict_unknown_file_stages(file_size_mb),
        }
    }

    fn predict_pdf_stages(file_size_mb: f64, pages: u32) -> Vec<StageResourceNeed> {
        vec![
            // 下载阶段
            StageResourceNeed {
                stage: ProcessingStage::Download,
                memory_mb: (file_size_mb * 1.5) as u32, // 下载缓存
                duration_seconds: Self::estimate_download_time(file_size_mb),
                cpu_intensive: false,
                io_intensive: true,
                concurrency_recommendation: 20,
            },
            // PDF转换阶段 - 最关键的瓶颈
            StageResourceNeed {
                stage: ProcessingStage::PdfConvert,
                memory_mb: Self::estimate_pdf_convert_memory(pages),
                duration_seconds: Self::estimate_pdf_convert_time(pages),
                cpu_intensive: true,
                io_intensive: false,
                concurrency_recommendation: Self::recommend_pdf_convert_concurrency(pages),
            },
            // OCR处理阶段
            StageResourceNeed {
                stage: ProcessingStage::OcrProcess,
                memory_mb: Self::estimate_ocr_memory(pages),
                duration_seconds: Self::estimate_ocr_time(pages),
                cpu_intensive: true,
                io_intensive: false,
                concurrency_recommendation: 8,
            },
            // 存储阶段
            StageResourceNeed {
                stage: ProcessingStage::Storage,
                memory_mb: 100,
                duration_seconds: 15,
                cpu_intensive: false,
                io_intensive: true,
                concurrency_recommendation: 15,
            },
        ]
    }

    fn predict_image_stages(file_size_mb: f64) -> Vec<StageResourceNeed> {
        vec![
            StageResourceNeed {
                stage: ProcessingStage::Download,
                memory_mb: (file_size_mb * 1.2) as u32,
                duration_seconds: Self::estimate_download_time(file_size_mb),
                cpu_intensive: false,
                io_intensive: true,
                concurrency_recommendation: 20,
            },
            StageResourceNeed {
                stage: ProcessingStage::OcrProcess,
                memory_mb: 600, // 图片OCR相对固定
                duration_seconds: 20,
                cpu_intensive: true,
                io_intensive: false,
                concurrency_recommendation: 8,
            },
            StageResourceNeed {
                stage: ProcessingStage::Storage,
                memory_mb: 50,
                duration_seconds: 8,
                cpu_intensive: false,
                io_intensive: true,
                concurrency_recommendation: 15,
            },
        ]
    }

    fn predict_unknown_file_stages(file_size_mb: f64) -> Vec<StageResourceNeed> {
        // 保守估算
        vec![
            StageResourceNeed {
                stage: ProcessingStage::Download,
                memory_mb: (file_size_mb * 2.0) as u32, // 保守的内存估算
                duration_seconds: Self::estimate_download_time(file_size_mb) * 2,
                cpu_intensive: false,
                io_intensive: true,
                concurrency_recommendation: 10,
            },
            StageResourceNeed {
                stage: ProcessingStage::OcrProcess,
                memory_mb: 1000, // 保守估算
                duration_seconds: 60,
                cpu_intensive: true,
                io_intensive: false,
                concurrency_recommendation: 4,
            },
        ]
    }

    /// PDF转换内存需求估算
    fn estimate_pdf_convert_memory(pages: u32) -> u32 {
        // 内存需求模型：基础内存 + 每页动态内存
        let base_memory = 1536; // 1.5GB基础内存
        let per_page_memory = match pages {
            0..=10 => 150,  // 小文档：每页150MB
            11..=50 => 100, // 中等文档：每页100MB
            51..=100 => 80, // 大文档：每页80MB
            _ => 60,        // 超大文档：每页60MB
        };

        let calculated = base_memory + (pages * per_page_memory);
        // 限制在8GB以内
        calculated.min(8192)
    }

    /// PDF转换时间估算
    fn estimate_pdf_convert_time(pages: u32) -> u32 {
        // 时间模型：基础时间 + 每页处理时间
        let base_time = 10; // 10秒基础时间
        let per_page_time = match pages {
            0..=20 => 5,   // 小文档：每页5秒
            21..=50 => 4,  // 中等文档：每页4秒
            51..=100 => 3, // 大文档：每页3秒
            _ => 2,        // 超大文档：每页2秒（批处理优化）
        };

        base_time + (pages * per_page_time)
    }

    /// OCR内存需求估算
    fn estimate_ocr_memory(pages: u32) -> u32 {
        // OCR内存相对固定，主要取决于并行处理的页数
        let base_memory = 400; // 400MB基础内存
        let batch_memory = 200; // 每批额外200MB
        let batch_size = 3; // 每批处理3页
        let batches = (pages + batch_size - 1) / batch_size;

        base_memory + (batches.min(8) * batch_memory) // 最多8个批次并行
    }

    /// OCR时间估算
    fn estimate_ocr_time(pages: u32) -> u32 {
        // OCR时间主要取决于页数和并发度
        let per_page_time = 2; // 每页2秒（并发处理）
        let batch_size = 3;
        let concurrent_batches = 8;

        let batches = (pages + batch_size - 1) / batch_size;
        let parallel_time = (batches + concurrent_batches - 1) / concurrent_batches;

        parallel_time * per_page_time * batch_size
    }

    /// PDF转换并发度推荐
    fn recommend_pdf_convert_concurrency(pages: u32) -> u32 {
        match pages {
            0..=10 => 3,   // 小文档：3并发
            11..=30 => 2,  // 中等文档：2并发
            31..=100 => 1, // 大文档：1并发
            _ => 1,        // 超大文档：1并发
        }
    }

    /// 下载时间估算
    fn estimate_download_time(file_size_mb: f64) -> u32 {
        // 假设平均下载速度2MB/s，加上网络延迟
        let base_time = 5; // 5秒基础时间
        let download_time = (file_size_mb / 2.0).ceil() as u32;
        base_time + download_time
    }

    /// 风险级别评估
    fn assess_risk_level(peak_memory_mb: u32, pages: u32, file_type: &str) -> RiskLevel {
        match (peak_memory_mb, pages, file_type) {
            (0..=2047, _, _) => RiskLevel::Low,
            (2048..=4095, 0..=50, _) => RiskLevel::Medium,
            (2048..=4095, 51.., _) => RiskLevel::High,
            (4096..=6143, _, _) => RiskLevel::High,
            (6144.., _, _) => RiskLevel::Critical,
        }
    }

    /// 生成执行建议
    fn generate_execution_recommendation(
        peak_memory_mb: u32,
        risk_level: &RiskLevel,
        pages: u32,
    ) -> ExecutionRecommendation {
        match risk_level {
            RiskLevel::Low => ExecutionRecommendation::Execute,
            RiskLevel::Medium => ExecutionRecommendation::Execute,
            RiskLevel::High => {
                if pages > 200 {
                    ExecutionRecommendation::Split // 建议分割处理
                } else {
                    ExecutionRecommendation::ExecuteWithCaution
                }
            }
            RiskLevel::Critical => {
                if pages > 500 {
                    ExecutionRecommendation::Reject // 拒绝执行
                } else if pages > 200 {
                    ExecutionRecommendation::Split // 强烈建议分割
                } else {
                    ExecutionRecommendation::Defer // 延迟执行
                }
            }
        }
    }

    /// 检查系统是否能安全处理该任务
    pub fn can_system_handle_task(profile: &TaskResourceProfile) -> TaskExecutability {
        let mut system = sysinfo::System::new_all();
        system.refresh_memory();

        let available_memory_gb = (system.available_memory() / 1024 / 1024 / 1024) as u32;
        let required_memory_gb = (profile.peak_memory_mb as f64 / 1024.0).ceil() as u32;

        // 预留4GB安全缓冲
        let safety_buffer_gb = 4;
        let can_execute = available_memory_gb > required_memory_gb + safety_buffer_gb;

        let safety_margin_gb = available_memory_gb.saturating_sub(required_memory_gb);

        let recommendation = if can_execute {
            match profile.risk_level {
                RiskLevel::Low => ExecutionRecommendation::Execute,
                RiskLevel::Medium => ExecutionRecommendation::Execute,
                RiskLevel::High => ExecutionRecommendation::ExecuteWithCaution,
                RiskLevel::Critical => ExecutionRecommendation::Defer,
            }
        } else {
            if profile.estimated_pages > 100 {
                ExecutionRecommendation::Split
            } else {
                ExecutionRecommendation::Defer
            }
        };

        let estimated_wait_time = if can_execute {
            0
        } else {
            // 根据当前系统负载估算等待时间
            match available_memory_gb.saturating_sub(required_memory_gb + safety_buffer_gb) {
                0..=2 => 600, // 10分钟
                3..=4 => 300, // 5分钟
                _ => 120,     // 2分钟
            }
        };

        let bottleneck_stage = Self::identify_bottleneck_stage(profile);
        let optimization_suggestions =
            Self::generate_optimization_suggestions(profile, available_memory_gb);

        TaskExecutability {
            can_execute,
            available_memory_gb,
            required_memory_gb,
            safety_margin_gb,
            recommendation,
            estimated_wait_time_seconds: estimated_wait_time,
            bottleneck_stage,
            optimization_suggestions,
        }
    }

    /// 识别瓶颈阶段
    fn identify_bottleneck_stage(profile: &TaskResourceProfile) -> Option<ProcessingStage> {
        profile
            .predicted_stages
            .iter()
            .max_by_key(|stage| stage.memory_mb)
            .map(|stage| stage.stage.clone())
    }

    /// 生成优化建议
    fn generate_optimization_suggestions(
        profile: &TaskResourceProfile,
        available_memory_gb: u32,
    ) -> Vec<String> {
        let mut suggestions = Vec::new();

        if profile.peak_memory_mb > (available_memory_gb * 1024 / 2) {
            suggestions.push("建议在系统空闲时处理，或考虑增加系统内存".to_string());
        }

        if profile.estimated_pages > 100 {
            suggestions.push("建议将大文档分割成小块处理，提高处理效率".to_string());
        }

        if matches!(
            profile.execution_recommendation,
            ExecutionRecommendation::Defer
        ) {
            suggestions.push("建议等待其他任务完成后再处理此文件".to_string());
        }

        // 根据风险级别给出具体建议
        match profile.risk_level {
            RiskLevel::Critical => {
                suggestions.push("高风险任务：建议启用实时内存监控".to_string());
                suggestions.push("考虑使用流式处理减少内存占用".to_string());
            }
            RiskLevel::High => {
                suggestions.push("中高风险任务：建议监控系统资源使用情况".to_string());
            }
            _ => {}
        }

        suggestions
    }

    /// 获取处理建议摘要
    pub fn get_processing_recommendation(
        profile: &TaskResourceProfile,
    ) -> ProcessingRecommendation {
        let executability = Self::can_system_handle_task(profile);

        ProcessingRecommendation {
            should_process: executability.can_execute,
            priority_level: Self::calculate_priority_level(profile),
            suggested_concurrency: Self::suggest_optimal_concurrency(profile),
            memory_requirements: MemoryRequirements {
                peak_mb: profile.peak_memory_mb,
                sustained_mb: profile
                    .predicted_stages
                    .iter()
                    .filter(|s| s.stage != ProcessingStage::PdfConvert)
                    .map(|s| s.memory_mb)
                    .max()
                    .unwrap_or(500),
                safety_buffer_mb: 2048, // 2GB安全缓冲
            },
            time_estimates: TimeEstimates {
                total_seconds: profile.total_estimated_duration_seconds,
                bottleneck_stage_seconds: profile
                    .predicted_stages
                    .iter()
                    .map(|s| s.duration_seconds)
                    .max()
                    .unwrap_or(0),
            },
            optimization_tips: executability.optimization_suggestions,
        }
    }

    fn calculate_priority_level(profile: &TaskResourceProfile) -> String {
        match (&profile.risk_level, profile.file_size_mb) {
            (RiskLevel::Low, size) if size < 10.0 => "high".to_string(),
            (RiskLevel::Low | RiskLevel::Medium, _) => "medium".to_string(),
            (RiskLevel::High, _) => "low".to_string(),
            (RiskLevel::Critical, _) => "deferred".to_string(),
        }
    }

    fn suggest_optimal_concurrency(profile: &TaskResourceProfile) -> HashMap<String, u32> {
        let mut suggestions = HashMap::new();

        for stage in &profile.predicted_stages {
            let stage_name = format!("{:?}", stage.stage).to_lowercase();
            suggestions.insert(stage_name, stage.concurrency_recommendation);
        }

        suggestions
    }
}

/// 处理建议汇总
#[derive(Debug, Serialize)]
pub struct ProcessingRecommendation {
    pub should_process: bool,
    pub priority_level: String, // "high", "medium", "low", "deferred"
    pub suggested_concurrency: HashMap<String, u32>,
    pub memory_requirements: MemoryRequirements,
    pub time_estimates: TimeEstimates,
    pub optimization_tips: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct MemoryRequirements {
    pub peak_mb: u32,
    pub sustained_mb: u32,
    pub safety_buffer_mb: u32,
}

#[derive(Debug, Serialize)]
pub struct TimeEstimates {
    pub total_seconds: u32,
    pub bottleneck_stage_seconds: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pdf_resource_prediction() {
        let profile = TaskResourcePredictor::predict_task_resources(
            10 * 1024 * 1024, // 10MB PDF
            "PDF",
        );

        assert_eq!(profile.file_type, "PDF");
        assert!(profile.estimated_pages > 0);
        assert!(profile.peak_memory_mb > 1000);
        assert_eq!(profile.predicted_stages.len(), 4);
    }

    #[test]
    fn test_image_resource_prediction() {
        let profile = TaskResourcePredictor::predict_task_resources(
            2 * 1024 * 1024, // 2MB image
            "JPG",
        );

        assert_eq!(profile.file_type, "JPG");
        assert_eq!(profile.estimated_pages, 1);
        assert!(profile.peak_memory_mb < 1000);
        assert_eq!(profile.predicted_stages.len(), 3);
    }

    #[test]
    fn test_risk_assessment() {
        // 测试低风险文档
        let small_pdf = TaskResourcePredictor::predict_task_resources(1024 * 1024, "PDF");
        assert!(matches!(
            small_pdf.risk_level,
            RiskLevel::Low | RiskLevel::Medium
        ));

        // 测试高风险文档
        let large_pdf = TaskResourcePredictor::predict_task_resources(100 * 1024 * 1024, "PDF");
        assert!(matches!(
            large_pdf.risk_level,
            RiskLevel::High | RiskLevel::Critical
        ));
    }
}
