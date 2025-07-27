# API接口文档

## 核心接口

### `/api/preview` - 材料预审

第三方系统调用此接口提交预审材料。

**请求**
```http
POST /api/preview
Content-Type: application/json
Authorization: Bearer <user_token>

{
  "user_id": "用户ID",
  "preview": {
    "request_id": "请求ID",
    "matter_id": "事项ID", 
    "matter_name": "事项名称",
    "material_data": [
      {
        "code": "材料编码",
        "name": "材料名称",
        "attachment_list": [
          {
            "attach_name": "文件名.pdf",
            "attach_url": "文件URL或base64数据"
          }
        ]
      }
    ]
  }
}
```

**响应**
```json
{
  "success": true,
  "errorCode": 200,
  "errorMsg": "",
  "data": {
    "preview_id": "预审ID",
    "preview_url": "预审结果页面URL"
  }
}
```

**处理流程**
1. 验证事项是否在映射表中
2. 验证用户身份（token + 用户ID）
3. OCR识别文档内容
4. 规则引擎匹配
5. 生成预审报告

## 辅助接口

### 预审状态查询
```http
GET /api/preview/status/{preview_id}
```

### 预审结果查看
```http
GET /api/preview/lookup/{third_party_request_id}
```

## 认证接口

### SSO登录跳转
```http
GET /api/sso/login?return_url=<返回URL>&request_id=<预审ID>
```

**参数说明**：
- `return_url`（可选）：登录成功后的返回URL
- `request_id`（可选）：待访问的预审记录ID

**功能**：
- 安全地跳转到第三方SSO登录
- 保存返回URL和预审ID到会话
- 自动重定向到配置的SSO登录地址

### SSO回调处理
```http
GET /api/sso/callback?ticketId=<票据ID>
```

**功能**：
- 处理第三方SSO登录成功回调
- 验证票据并创建用户会话
- 根据优先级重定向：预审页面 > 返回URL > 主页

### 认证状态检查
```http
GET /api/auth/status
```

**响应**：
```json
{
  "authenticated": true,
  "user": {
    "userId": "用户ID",
    "userName": "用户姓名",
    // ... 其他用户信息
  }
}
```

## 认证说明

- **生产环境**：使用SSO单点登录
- **测试环境**：支持测试用户登录
- 验证用户ID一致性
- 不一致时自动跳转SSO登录

## 测试接口

### 测试登录
```http
POST /api/test/login
{
  "username": "测试用户"
}
```

### 健康检查
```http
GET /api/health
```
