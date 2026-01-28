//! CSS样式管理模块
//! 集中管理报告生成的所有CSS样式

/// CSS样式管理器
pub struct CssStyleManager;

impl CssStyleManager {
    /// 获取完整报告CSS样式
    pub fn get_report_css() -> &'static str {
        r#"
        body { 
            font-family: 'Microsoft YaHei', Arial, sans-serif; 
            margin: 0; 
            padding: 20px; 
            line-height: 1.6;
            color: #333;
            background-color: #fff;
        }
        .report-title { 
            text-align: center; 
            color: #2c3e50; 
            border-bottom: 3px solid #3498db;
            padding-bottom: 10px;
            margin-bottom: 30px;
            font-size: 2.2em;
            font-weight: bold;
            page-break-after: avoid;
        }
        .report-header {
            margin-top: 0;
            padding-top: 10px;
            margin-bottom: 25px;
            break-inside: avoid;
        }
        .report-header h2 { margin-top: 12px; }
        .section { 
            margin: 30px 0; 
            padding: 20px;
            border: 1px solid #e0e0e0;
            border-radius: 8px;
            background: #fafafa;
            box-shadow: 0 2px 4px rgba(0,0,0,0.05);
        }
        .section h2 { 
            color: #2c3e50; 
            border-bottom: 2px solid #3498db;
            padding-bottom: 5px;
            margin-top: 0;
            font-size: 1.5em;
        }
        table { 
            border-collapse: collapse; 
            width: 100%;
            margin: 15px 0;
            background: white;
        }
        th, td { 
            padding: 12px 15px; 
            border: 1px solid #ddd; 
            text-align: left;
            vertical-align: top;
        }
        th { 
            background-color: #3498db; 
            color: white; 
            font-weight: bold;
        }
        tr:nth-child(even) {
            background-color: #f9f9f9;
        }
        .summary-box {
            background: white;
            border-radius: 8px;
            padding: 20px;
            box-shadow: 0 2px 8px rgba(0,0,0,0.1);
            margin: 20px 0;
        }
        .summary-result {
            text-align: center;
            margin-bottom: 20px;
            padding: 15px;
            border-radius: 6px;
        }
        .summary-result h3 {
            margin: 0;
            font-size: 1.8em;
        }
        .summary-stats {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
            gap: 10px;
            margin: 20px 0;
        }
        .summary-stats p {
            margin: 5px 0;
            padding: 10px;
            background: #f8f9fa;
            border-radius: 4px;
        }
        .result-success { 
            color: #27ae60; 
            background-color: #d5f4e6;
        }
        .result-warning { 
            color: #f39c12; 
            background-color: #fef9e7;
        }
        .result-error { 
            color: #e74c3c; 
            background-color: #fdf2f2;
        }
        .text-success { color: #27ae60; font-weight: bold; }
        .text-warning { color: #f39c12; font-weight: bold; }
        .text-error { color: #e74c3c; font-weight: bold; }
        .material-item {
            margin: 15px 0;
            padding: 15px;
            border-radius: 6px;
            border-left: 4px solid #3498db;
            background: white;
            box-shadow: 0 1px 3px rgba(0,0,0,0.1);
        }
        .material-item h3 {
            margin-top: 0;
            color: #2c3e50;
        }
        .material-success { 
            background: #d5f4e6; 
            border-left-color: #27ae60;
        }
        .material-error { 
            background: #fdf2f2; 
            border-left-color: #e74c3c;
        }
        .material-details p {
            margin: 8px 0;
        }
        .attachments {
            margin-top: 10px;
            display: flex;
            flex-wrap: wrap;
            gap: 12px;
        }
        .attachment-item {
            background: #ffffff;
            border: 1px solid #e0e0e0;
            border-radius: 6px;
            padding: 10px;
            max-width: 220px;
            box-shadow: 0 1px 3px rgba(0,0,0,0.08);
            display: flex;
            flex-direction: column;
            gap: 6px;
        }
        .attachment-name {
            font-weight: bold;
            color: #1f6feb;
            word-break: break-all;
        }
        .attachment-link {
            text-decoration: none;
        }
        .attachment-link:hover {
            text-decoration: underline;
        }
        .attachment-meta {
            font-size: 0.85em;
            color: #666;
        }
        .attachment-preview img {
            width: 100%;
            border-radius: 4px;
            border: 1px solid #ddd;
            box-shadow: 0 1px 2px rgba(0,0,0,0.1);
        }
        .suggestions {
            margin-top: 15px;
            padding: 12px;
            background: #fff3cd;
            border: 1px solid #ffeaa7;
            border-radius: 4px;
        }
        .suggestions h4 {
            margin-top: 0;
            color: #856404;
        }
        .suggestions ul {
            margin-bottom: 0;
        }
        .footer {
            margin-top: 40px;
            padding-top: 20px;
            border-top: 2px solid #ddd;
            text-align: center;
            color: #666;
            font-size: 0.9em;
        }
        @media print {
            body { margin: 0; padding: 15px; }
            .section { break-inside: avoid; }
            .material-item { break-inside: avoid; }
            .report-header { break-inside: avoid; page-break-after: avoid; }
        }
        "#
    }

    /// 获取简化版CSS样式
    pub fn get_simple_css() -> &'static str {
        r#"
        body { 
            font-family: 'Microsoft YaHei', Arial, sans-serif; 
            margin: 20px; 
            line-height: 1.6;
            color: #333;
        }
        h1 { 
            color: #2c3e50; 
            border-bottom: 2px solid #3498db;
            padding-bottom: 10px;
            font-size: 2em;
        }
        h2 { 
            color: #34495e; 
            margin-top: 25px;
            font-size: 1.5em;
        }
        p {
            margin: 10px 0;
        }
        strong {
            color: #2c3e50;
        }
        ul { 
            padding-left: 20px; 
        }
        li { 
            margin: 8px 0; 
            padding: 5px 0;
        }
        @media print {
            body { margin: 10px; }
        }
        "#
    }

    /// 获取对比报告CSS样式
    pub fn get_comparison_css() -> &'static str {
        r#"
        body { 
            font-family: 'Microsoft YaHei', Arial, sans-serif; 
            margin: 0; 
            padding: 20px; 
            line-height: 1.6;
            color: #333;
        }
        h1 { 
            text-align: center; 
            color: #2c3e50; 
            border-bottom: 3px solid #3498db;
            padding-bottom: 10px;
            margin-bottom: 30px;
        }
        .comparison-container {
            display: grid;
            grid-template-columns: 1fr 1fr;
            gap: 20px;
            margin: 20px 0;
        }
        .original-section, .updated-section {
            padding: 15px;
            border: 1px solid #ddd;
            border-radius: 8px;
            background: #fafafa;
        }
        .original-section h2 {
            color: #e74c3c;
            border-bottom: 2px solid #e74c3c;
        }
        .updated-section h2 {
            color: #27ae60;
            border-bottom: 2px solid #27ae60;
        }
        .material-item {
            margin: 10px 0;
            padding: 10px;
            border-radius: 4px;
            border-left: 3px solid #3498db;
            background: white;
        }
        .material-success { 
            background: #d5f4e6; 
            border-left-color: #27ae60;
        }
        .material-error { 
            background: #fdf2f2; 
            border-left-color: #e74c3c;
        }
        @media print {
            .comparison-container {
                grid-template-columns: 1fr;
            }
        }
        "#
    }

    /// 获取移动端友好的CSS样式
    pub fn get_mobile_css() -> &'static str {
        r#"
        body { 
            font-family: 'Microsoft YaHei', Arial, sans-serif; 
            margin: 0; 
            padding: 10px; 
            line-height: 1.5;
            color: #333;
            font-size: 14px;
        }
        .report-title { 
            text-align: center; 
            color: #2c3e50; 
            border-bottom: 2px solid #3498db;
            padding-bottom: 8px;
            margin-bottom: 20px;
            font-size: 1.5em;
        }
        .section { 
            margin: 15px 0; 
            padding: 10px;
            border: 1px solid #e0e0e0;
            border-radius: 6px;
            background: #fafafa;
        }
        .section h2 { 
            color: #2c3e50; 
            border-bottom: 1px solid #3498db;
            padding-bottom: 3px;
            font-size: 1.2em;
            margin-top: 0;
        }
        table { 
            width: 100%;
            border-collapse: collapse;
            font-size: 12px;
        }
        th, td { 
            padding: 8px 6px; 
            border: 1px solid #ddd; 
            text-align: left;
        }
        .summary-stats {
            display: block;
        }
        .summary-stats p {
            margin: 5px 0;
            padding: 8px;
            background: #f8f9fa;
            border-radius: 3px;
        }
        .material-item {
            margin: 10px 0;
            padding: 10px;
            border-radius: 4px;
            border-left: 3px solid #3498db;
        }
        "@media (max-width: 768px) {
            body { padding: 5px; font-size: 13px; }
            .report-title { font-size: 1.3em; }
            th, td { padding: 6px 4px; font-size: 11px; }
        }
        "#
    }

    /// 获取深色主题CSS样式
    pub fn get_dark_theme_css() -> &'static str {
        r#"
        body { 
            font-family: 'Microsoft YaHei', Arial, sans-serif; 
            margin: 0; 
            padding: 20px; 
            line-height: 1.6;
            color: #e0e0e0;
            background-color: #1a1a1a;
        }
        .report-title { 
            text-align: center; 
            color: #4fc3f7; 
            border-bottom: 3px solid #4fc3f7;
            padding-bottom: 10px;
            margin-bottom: 30px;
        }
        .section { 
            margin: 30px 0; 
            padding: 20px;
            border: 1px solid #404040;
            border-radius: 8px;
            background: #2d2d2d;
        }
        .section h2 { 
            color: #4fc3f7; 
            border-bottom: 2px solid #4fc3f7;
            padding-bottom: 5px;
        }
        table { 
            border-collapse: collapse; 
            width: 100%;
            margin: 15px 0;
            background: #333;
        }
        th, td { 
            padding: 12px 15px; 
            border: 1px solid #555; 
            text-align: left;
        }
        th { 
            background-color: #4fc3f7; 
            color: #1a1a1a; 
            font-weight: bold;
        }
        tr:nth-child(even) {
            background-color: #3a3a3a;
        }
        .summary-box {
            background: #333;
            border-radius: 8px;
            padding: 20px;
            box-shadow: 0 2px 8px rgba(0,0,0,0.3);
        }
        .result-success { 
            color: #4caf50; 
            background-color: #1b4332;
        }
        .result-warning { 
            color: #ff9800; 
            background-color: #3d2914;
        }
        .result-error { 
            color: #f44336; 
            background-color: #4a1a1a;
        }
        .material-item {
            margin: 15px 0;
            padding: 15px;
            border-radius: 6px;
            border-left: 4px solid #4fc3f7;
            background: #333;
        }
        .suggestions {
            background: #3d2914;
            border: 1px solid #ff9800;
            color: #ffcc02;
        }
        .footer {
            border-top: 2px solid #404040;
            color: #999;
        }
        "#
    }
}

/// 公共样式访问函数（保持向后兼容）
pub fn get_report_css() -> &'static str {
    CssStyleManager::get_report_css()
}

pub fn get_simple_css() -> &'static str {
    CssStyleManager::get_simple_css()
}

pub fn get_comparison_css() -> &'static str {
    CssStyleManager::get_comparison_css()
}

pub fn get_mobile_css() -> &'static str {
    CssStyleManager::get_mobile_css()
}

pub fn get_dark_theme_css() -> &'static str {
    CssStyleManager::get_dark_theme_css()
}
