# 配置说明

## 主配置文件

配置文件：`config.yaml`

## 基本配置

```yaml
# 服务配置
host: "http://127.0.0.1"
port: 31101
preview_url: "http://127.0.0.1:31101"
```

## 数据库配置

```yaml
# 达梦数据库（生产环境）
DMSql:
  DATABASE_HOST: "192.168.1.100"
  DATABASE_PORT: "5236"
  DATABASE_USER: "SYSDBA"
  DATABASE_PASSWORD: "SYSDBA"
  DATABASE_NAME: "OCR_DB"

# SQLite（开发环境，HOST为空时自动使用）
DMSql:
  DATABASE_HOST: ""
```

## 存储配置

```yaml
# 阿里云OSS（生产环境）
zhzwdt-oss:
  root: "ocr-files"
  bucket: "your-bucket"
  server_url: "https://your-oss-endpoint.com"
  AccessKey: "your-access-key"
  AccessKey Secret: "your-secret"

# 本地存储（开发环境，AccessKey为空时自动使用）
zhzwdt-oss:
  AccessKey: ""
```

## SSO登录配置

```yaml
login:
  sso_login_url: "https://sso.example.com/login"
  access_token_url: "https://sso.example.com/token"
  get_user_info_url: "https://sso.example.com/userinfo"
  access_key: "your-sso-access-key"
  secret_key: "your-sso-secret-key"
  use_callback: false
```

## 测试模式配置

```yaml
test_mode:
  enabled: false              # 是否启用测试模式
  auto_login: true            # 自动登录
  mock_ocr: true              # 模拟OCR结果
  mock_delay: 500             # 模拟延迟(ms)
  
  test_user:
    id: "test_user_001"
    username: "测试用户"
    email: "test@example.com"
```

## 故障转移配置

```yaml
failover:
  database:
    enabled: true             # 启用数据库故障转移
    health_check_interval: 30 # 健康检查间隔(秒)
    max_retries: 3            # 最大重试次数
    fallback_to_local: true   # 降级到本地SQLite
    
  storage:
    enabled: true             # 启用存储故障转移
    auto_switch_to_local: true # 自动切换到本地存储
    sync_when_recovered: true  # 恢复后同步数据
```

## 主题规则配置

主题配置文件：`themes.json`

```json
{
  "themes": [
    {
      "id": "theme_001",
      "name": "工程渣土准运证核准",
      "rule_file": "rules/theme_001.json",
      "enabled": true
    }
  ]
}
```

事项映射文件：`matter-theme-mapping.json`

```json
{
  "mappings": [
    {
      "matterId": "MATTER_16570147206221001",
      "matterName": "杭州市工程渣土准运证核准申请",
      "themeId": "theme_001"
    }
  ]
}
```

## 日志配置

```yaml
logging:
  level: "info"               # 日志级别
  file:
    enabled: true             # 启用文件日志
    directory: "runtime/logs" # 日志目录
    retention_days: 7         # 保留天数
```

## 环境变量

可以通过环境变量覆盖配置：

```bash
export OCR_MODE=test          # 测试模式
export OCR_PORT=31101         # 服务端口
export OCR_LOG_LEVEL=debug    # 日志级别
```
