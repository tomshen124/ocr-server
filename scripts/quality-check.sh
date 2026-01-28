#!/bin/bash
# 代码质量检查脚本 - 集成Clippy和格式化检查
# 用法: ./scripts/quality-check.sh [--fix]

set -e

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_ROOT"

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# 参数解析
FIX_MODE=false
if [[ "$1" == "--fix" ]]; then
    FIX_MODE=true
fi

echo -e "${BLUE}=====================================${NC}"
echo -e "${BLUE}🔍 OCR服务器代码质量检查${NC}"
echo -e "${BLUE}=====================================${NC}"
echo ""

# 1. 代码格式检查
echo -e "${YELLOW}📝 Step 1: 代码格式检查 (rustfmt)${NC}"
if [ "$FIX_MODE" = true ]; then
    echo "正在自动修复格式问题..."
    cargo fmt --all
    echo -e "${GREEN}✅ 代码格式已修复${NC}"
else
    if cargo fmt --all -- --check; then
        echo -e "${GREEN}✅ 代码格式检查通过${NC}"
    else
        echo -e "${RED}❌ 代码格式不符合规范，运行 $0 --fix 自动修复${NC}"
        exit 1
    fi
fi
echo ""

# 2. Clippy代码质量检查
echo -e "${YELLOW}🔧 Step 2: Clippy代码质量检查${NC}"
echo "检查规则:"
echo "  - unwrap/expect使用"
echo "  - 不必要的clone"
echo "  - 潜在的panic"
echo "  - 未使用的结果"
echo ""

# 基础Clippy检查
if cargo clippy --all-targets --all-features -- \
    -W clippy::unwrap_used \
    -W clippy::expect_used \
    -W clippy::clone_on_copy \
    -W clippy::clone_on_ref_ptr \
    -W unused_must_use \
    -W clippy::panic \
    -W clippy::todo \
    -W clippy::unimplemented \
    -A clippy::too_many_arguments \
    -A clippy::type_complexity; then
    echo -e "${GREEN}✅ Clippy检查通过${NC}"
else
    echo -e "${RED}❌ Clippy发现代码质量问题${NC}"
    echo -e "${YELLOW}💡 提示: 查看上方输出的详细信息进行修复${NC}"
    exit 1
fi
echo ""

# 3. 编译检查
echo -e "${YELLOW}🏗️  Step 3: 编译检查${NC}"
if cargo check --all-targets --all-features; then
    echo -e "${GREEN}✅ 编译检查通过${NC}"
else
    echo -e "${RED}❌ 编译失败${NC}"
    exit 1
fi
echo ""

# 4. 测试检查
echo -e "${YELLOW}🧪 Step 4: 单元测试${NC}"
if cargo test --all --no-fail-fast; then
    echo -e "${GREEN}✅ 所有测试通过${NC}"
else
    echo -e "${RED}❌ 部分测试失败${NC}"
    exit 1
fi
echo ""

# 5. 依赖审计（可选）
echo -e "${YELLOW}🔐 Step 5: 依赖安全审计 (cargo-audit)${NC}"
if command -v cargo-audit &> /dev/null; then
    if cargo audit; then
        echo -e "${GREEN}✅ 依赖安全审计通过${NC}"
    else
        echo -e "${YELLOW}⚠️  发现安全漏洞，请检查上方输出${NC}"
    fi
else
    echo -e "${YELLOW}⚠️  cargo-audit未安装，跳过依赖审计${NC}"
    echo -e "${YELLOW}💡 安装方法: cargo install cargo-audit${NC}"
fi
echo ""

# 6. 代码统计
echo -e "${YELLOW}📊 Step 6: 代码统计${NC}"
echo "文件统计:"
find src -name "*.rs" | wc -l | xargs echo "  Rust文件数:"
find src -name "*.rs" -exec wc -l {} + | tail -1 | awk '{print "  总代码行数: " $1}'
echo ""

# 7. TODO/FIXME检查
echo -e "${YELLOW}📋 Step 7: TODO/FIXME标记检查${NC}"
TODO_COUNT=$(find src -name "*.rs" -exec grep -n "TODO\|FIXME\|XXX" {} + 2>/dev/null | wc -l | tr -d ' ')
if [ "$TODO_COUNT" -gt 0 ]; then
    echo -e "${YELLOW}⚠️  发现 $TODO_COUNT 个TODO/FIXME/XXX标记${NC}"
    echo "详细列表:"
    find src -name "*.rs" -exec grep -Hn "TODO\|FIXME\|XXX" {} + 2>/dev/null | head -20
    if [ "$TODO_COUNT" -gt 20 ]; then
        echo "  ... (显示前20个，总共${TODO_COUNT}个)"
    fi
else
    echo -e "${GREEN}✅ 无TODO/FIXME/XXX标记${NC}"
fi
echo ""

# 最终总结
echo -e "${BLUE}=====================================${NC}"
echo -e "${GREEN}✅ 代码质量检查完成！${NC}"
echo -e "${BLUE}=====================================${NC}"
echo ""
echo "质量评分:"
echo "  ✅ 代码格式: 通过"
echo "  ✅ Clippy检查: 通过"
echo "  ✅ 编译检查: 通过"
echo "  ✅ 单元测试: 通过"
if [ "$TODO_COUNT" -gt 0 ]; then
    echo -e "  ${YELLOW}⚠️  代码标记: $TODO_COUNT 个待处理${NC}"
else
    echo "  ✅ 代码标记: 无"
fi
echo ""
echo -e "${GREEN}🎉 系统代码质量达标，可以提交！${NC}"
