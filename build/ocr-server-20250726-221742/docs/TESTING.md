# 🧪 OCR预审系统测试指南

## 📋 测试模式配置

### 1. 配置文件设置

在 `config/config.yaml` 中启用测试模式：

```yaml
# 开发调试配置
debug:
  enabled: true
  enable_mock_login: true        # 启用模拟登录
  mock_login_warning: true       # 显示安全警告
  tools_enabled:
    api_test: true
    mock_login: true
    preview_demo: true
    flow_test: true
    system_monitor: true

# 测试模式配置
test_mode:
  enabled: true                  # 启用测试模式
  auto_login: true              # 自动登录
  mock_ocr: true                # 模拟OCR结果
  mock_delay: 500               # 模拟处理延迟
  
  # 测试用户配置
  test_user:
    id: "test_user_001"
    username: "测试用户"
    email: "test@example.com"
```

### 2. 环境变量（可选）

```bash
# 启用测试模式
export OCR_TEST_MODE=true

# 设置日志级别
export RUST_LOG=debug

# 禁用生产环境检查
export RUST_ENV=development
```

## 🔧 测试工具使用

### 1. 测试工具页面

访问测试工具页面：
```
http://localhost:31101/static/test-tools.html
```

**功能包括：**
- 🔑 模拟登录测试
- 📋 预审数据测试  
- 🔗 第三方接口测试
- 📊 系统状态监控

### 2. 快速测试流程

#### A. 模拟登录测试
```javascript
// 使用默认测试用户
POST /api/dev/mock_login
{
  "userId": "1472176",
  "userName": "张三"
}
```

#### B. 预审页面测试
```
# 测试模式访问预审页面
http://localhost:31101/static/preview.html?test=true

# 带请求ID的测试
http://localhost:31101/static/preview.html?test=true&requestId=test_001
```

#### C. 第三方回调测试
```javascript
POST /api/third-party/callback
{
  "matterId": "101104353",
  "matterName": "工程渣土准运证核准",
  "agentInfo": {
    "certificateType": "ID_CARD", 
    "userId": "1472176"
  },
  "subjectInfo": {
    "certificateType": "ID_CARD",
    "userId": "1472176"
  }
}
```

## 📊 测试场景

### 1. 基础功能测试

#### 场景1：模拟登录流程
1. 访问测试工具页面
2. 点击"快速模拟登录"
3. 验证登录状态
4. 检查会话信息

#### 场景2：预审数据获取
1. 确保已登录
2. 测试获取模拟预审数据
3. 验证数据格式和内容
4. 检查UI渲染效果

#### 场景3：完整预审流程
1. 模拟第三方系统发送预审请求
2. 系统自动处理OCR识别
3. 应用规则引擎评估
4. 生成预审结果页面
5. 用户访问预审结果

### 2. 边界情况测试

#### 场景4：无认证访问
1. 清除浏览器会话
2. 直接访问预审页面
3. 验证访问控制机制
4. 测试重定向逻辑

#### 场景5：数据异常处理
1. 发送格式错误的JSON数据
2. 测试缺少必需字段的请求
3. 验证错误处理和用户提示
4. 检查系统稳定性

#### 场景6：并发访问测试
1. 同时发送多个预审请求
2. 测试系统并发处理能力
3. 验证数据隔离性
4. 检查性能表现

## 🎯 测试用户数据

### 预定义测试用户

| 用户ID | 用户名 | 用途 |
|--------|--------|------|
| 1472176 | 张三 | 默认测试用户 |
| 8afac0cc580b07c701581aefd2435265 | 李四 | 长ID测试 |
| test_user_001 | 测试用户 | 通用测试 |

### 测试事项数据

| 事项ID | 事项名称 | 主题ID |
|--------|----------|--------|
| 101104353 | 工程渣土准运证核准 | theme_001 |
| 101105083 | 设置其他户外广告设施和招牌、指示牌备案 | theme_002 |
| 101303167 | 利用广场等公共场所举办文化、商业等活动许可 | theme_003 |

## 🔍 测试验证点

### 1. 功能验证
- ✅ 模拟登录成功创建会话
- ✅ 预审数据正确获取和转换
- ✅ UI正确渲染材料检查结果
- ✅ 状态标识准确显示
- ✅ 用户信息正确映射

### 2. 安全验证
- ✅ 未认证用户无法访问预审页面
- ✅ 跨用户数据隔离
- ✅ 会话超时处理
- ✅ 输入数据验证

### 3. 性能验证
- ✅ 页面加载时间 < 3秒
- ✅ 数据获取响应时间 < 2秒
- ✅ 并发处理能力
- ✅ 内存使用稳定

## 🚨 注意事项

### 安全警告
- ⚠️ **测试模式仅用于开发环境**
- ⚠️ **生产环境必须禁用所有测试功能**
- ⚠️ **模拟登录存在安全风险**
- ⚠️ **定期清理测试数据**

### 配置检查
```bash
# 检查当前配置
curl http://localhost:31101/api/config/frontend

# 检查系统健康状态
curl http://localhost:31101/api/health

# 检查认证状态
curl -b cookies.txt http://localhost:31101/api/auth/status
```

### 日志监控
```bash
# 查看实时日志
tail -f runtime/logs/ocr-*.log

# 查看错误日志
grep "ERROR" runtime/logs/ocr-*.log

# 查看测试相关日志
grep "🧪\|mock\|test" runtime/logs/ocr-*.log
```

## 📈 测试报告

### 测试结果记录
- 测试时间：
- 测试环境：
- 测试版本：
- 测试结果：
  - [ ] 模拟登录功能
  - [ ] 预审数据获取
  - [ ] UI渲染正确
  - [ ] 安全控制有效
  - [ ] 性能满足要求

### 问题记录
- 发现问题：
- 问题级别：
- 解决方案：
- 验证结果：

## 🔄 持续测试

### 自动化测试脚本
```bash
#!/bin/bash
# 自动化测试脚本示例

echo "🧪 开始自动化测试..."

# 1. 启动服务
./scripts/ocr-server.sh start

# 2. 等待服务就绪
sleep 5

# 3. 执行测试用例
curl -X POST http://localhost:31101/api/dev/mock_login \
  -H "Content-Type: application/json" \
  -d '{"userId":"test_user_001","userName":"测试用户"}'

# 4. 验证结果
curl http://localhost:31101/api/auth/status

echo "✅ 自动化测试完成"
```

### 集成测试
- 与CI/CD流水线集成
- 定期执行回归测试
- 性能基准测试
- 安全扫描测试
