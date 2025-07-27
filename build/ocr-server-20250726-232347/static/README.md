# 前端静态资源目录结构

## 📁 目录说明

```
static/
├── css/                    # 样式文件目录
│   └── style.css          # 主样式文件
├── js/                     # JavaScript 文件目录
│   ├── config.js          # 配置文件（图片路径映射等）
│   ├── script.js          # 主页面脚本
│   ├── preview-script.js  # 预审页面脚本
│   └── preview-data-api.js # 预审数据API
├── images/                 # 图片资源目录
│   ├── 智能预审_审核依据材料1.3.png
│   ├── 智能预审_已通过材料1.3.png
│   ├── 智能预审_文字hover1.3.png
│   ├── 智能预审_无审核依据材料1.3.png
│   ├── 智能预审_有审查点1.3.png
│   ├── 智能预审loading1.3.png
│   ├── 智能预审异常提示1.3.png
│   └── 预审通过1.3.png
├── index.html              # 主页面
└── preview.html            # 预审页面
```

## 🔧 文件说明

### HTML 文件
- **index.html**: 主页面，包含文件上传和主题选择功能
- **preview.html**: 预审结果页面，显示材料检查结果

### CSS 文件
- **css/style.css**: 包含所有页面的样式定义

### JavaScript 文件
- **js/config.js**: 配置文件，包含图片路径映射和基础配置
- **js/script.js**: 主页面的交互逻辑
- **js/preview-script.js**: 预审页面的交互逻辑和UI控制
- **js/preview-data-api.js**: 预审数据API，负责从后端获取和转换数据

### 图片资源
- **images/**: 存放所有UI相关的图片资源，主要用于材料预审结果展示

## 🚀 使用说明

1. 所有HTML文件中的资源引用都使用相对路径
2. JavaScript文件中的图片路径配置在 `js/config.js` 中统一管理
3. 新增样式请在 `css/style.css` 中添加
4. 新增脚本请在对应的 `js/` 目录下创建或修改

## 📝 注意事项

- 修改目录结构时，请同步更新HTML文件中的引用路径
- 图片路径配置在 `js/config.js` 中，修改图片时请更新配置
- 保持目录结构的整洁，按功能分类存放文件
