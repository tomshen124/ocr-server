#!/bin/bash
# Git pre-commit hook - 代码质量检查
# 自动安装: ln -sf ../../scripts/pre-commit.sh .git/hooks/pre-commit

set -e

echo "🔍 执行pre-commit代码质量检查..."

# 1. 格式检查
echo "📝 检查代码格式..."
if ! cargo fmt --all -- --check; then
    echo "❌ 代码格式不符合规范"
    echo "💡 运行 'cargo fmt' 自动格式化"
    exit 1
fi

# 2. 快速Clippy检查（仅检查修改的文件相关代码）
echo "🔧 执行Clippy检查..."
if ! cargo clippy --all-targets -- \
    -W clippy::unwrap_used \
    -W clippy::expect_used \
    -D warnings; then
    echo "❌ Clippy检查失败"
    echo "💡 修复上述警告后再提交"
    exit 1
fi

# 3. 编译检查
echo "🏗️  检查编译..."
if ! cargo check --quiet; then
    echo "❌ 编译失败"
    exit 1
fi

echo "✅ Pre-commit检查通过！"
exit 0
