# 前端风格统一实施任务

- [x] 1. 备份和清理现有静态文件
  - 创建static目录的备份，保存当前临时代码作为参考
  - 清理static目录中的临时文件，为新的前端代码做准备
  - _Requirements: 1.3, 1.4_

- [ ] 2. 复制和适配yjb_projet基础文件
  - [x] 2.1 复制yjb_projet的核心文件到static目录
    - 复制index.html、style.css、script.js、config.js到static/
    - 复制images目录到static/images/
    - 确保文件权限和路径正确
    - _Requirements: 1.1, 1.2_

  - [ ] 2.2 创建增强的预审页面
    - 基于yjb_projet/index.html创建static/preview.html
    - 集成生产环境需要的文件上传功能
    - 添加用户认证界面元素
    - 保持yjb_projet的视觉风格和布局
    - _Requirements: 1.1, 3.1, 3.2_

- [ ] 3. 实现JavaScript功能模块化
  - [ ] 3.1 创建统一配置管理模块
    - 实现static/js/unified-config.js，整合环境配置
    - 支持开发/生产环境的功能开关
    - 提供前端配置API接口
    - _Requirements: 3.3, 3.4_

  - [ ] 3.2 实现认证功能模块
    - 创建static/js/unified-auth.js处理用户认证
    - 集成SSO登录和会话管理
    - 实现用户信息显示和退出功能
    - _Requirements: 3.1, 3.2_

  - [ ] 3.3 实现预审管理模块
    - 创建static/js/preview-manager.js管理预审流程
    - 集成文件上传和主题选择功能
    - 实现进度显示和状态管理
    - _Requirements: 1.1, 3.2_

  - [ ] 3.4 实现结果展示模块
    - 创建static/js/preview-result.js处理结果显示
    - 实现材料列表和图片查看功能
    - 添加审查点标记和工具提示
    - _Requirements: 1.1, 1.2_

- [ ] 4. 优化静态文件路径配置
  - [ ] 4.1 扩展配置文件结构
    - 在config.yaml中添加static_files配置节
    - 定义base_path、fallback_paths等配置项
    - 添加auto_detect和enabled开关
    - _Requirements: 2.1, 2.2_

  - [ ] 4.2 重构静态文件路径处理逻辑
    - 修改src/api/mod.rs中的路径检测代码
    - 实现配置驱动的路径解析
    - 添加多路径备用机制和错误处理
    - 改进日志记录，提供清晰的路径信息
    - _Requirements: 2.1, 2.2, 2.3, 2.4_

- [ ] 5. 实现环境适配功能
  - [ ] 5.1 创建前端环境检测
    - 实现环境标识显示功能
    - 根据配置动态显示/隐藏功能模块
    - 添加Toast消息提示系统
    - _Requirements: 3.3, 3.4_

  - [ ] 5.2 实现功能开关机制
    - 创建功能开关配置接口
    - 实现前端功能的动态启用/禁用
    - 确保yjb_projet风格在所有模式下保持一致
    - _Requirements: 3.4, 3.5_

- [ ] 6. 更新构建和部署脚本
  - [ ] 6.1 修改生产环境构建脚本
    - 更新build-release-package.sh处理新的static结构
    - 确保yjb_projet风格的文件正确打包
    - 移除对临时代码的依赖
    - _Requirements: 4.1, 4.5_

  - [ ] 6.2 修改离线部署构建脚本
    - 更新build-offline-package.sh包含新的静态资源
    - 确保图片资源和样式文件完整打包
    - 验证部署包的文件结构正确性
    - _Requirements: 4.2, 4.5_

- [ ] 7. 实现错误处理和日志优化
  - [ ] 7.1 添加静态文件加载检测
    - 在服务启动时验证静态文件路径
    - 实现资源可用性检查
    - 添加详细的错误日志和修复建议
    - _Requirements: 2.3, 2.4_

  - [ ] 7.2 实现前端错误处理
    - 添加资源加载失败的检测和重试
    - 实现友好的错误提示界面
    - 提供降级到基本功能的机制
    - _Requirements: 2.3, 2.4_

- [ ] 8. 创建测试和验证
  - [ ] 8.1 实现前端功能测试
    - 创建页面渲染和功能完整性测试
    - 验证yjb_projet风格正确应用
    - 测试不同环境配置下的功能开关
    - _Requirements: 1.1, 1.2, 3.4_

  - [ ] 8.2 实现路径解析测试
    - 测试不同部署环境下的静态文件访问
    - 验证路径配置和备用机制
    - 测试错误处理和日志记录
    - _Requirements: 2.1, 2.2, 2.3, 2.4_

- [ ] 9. 文档更新和清理
  - [ ] 9.1 更新部署文档
    - 更新README.md说明新的前端结构
    - 创建前端开发和维护指南
    - 更新配置文件示例和说明
    - _Requirements: 4.3, 4.4_

  - [ ] 9.2 清理废弃文件
    - 标记或移除static中的临时代码文件
    - 更新.gitignore排除备份文件
    - 清理构建过程中的临时文件
    - _Requirements: 4.5_