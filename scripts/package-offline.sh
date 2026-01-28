#!/usr/bin/env bash
#
# OCR智能预审系统 - 生产环境离线部署包生成脚本
#
# 默认行为：在 build/ 目录下生成包含镜像、配置模板、证书、部署脚本与文档的离线包。
#
set -euo pipefail

BLUE='\033[0;34m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

log_info() { echo -e "${BLUE}[INFO]${NC} $1"; }
log_ok()   { echo -e "${GREEN}[✓]${NC} $1"; }
log_warn() { echo -e "${YELLOW}[!]${NC} $1"; }

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEFAULT_VERSION="$(cat "${ROOT_DIR}/VERSION" 2>/dev/null | tr -d '\n\r')"
if [[ -z "${DEFAULT_VERSION}" ]]; then
  DEFAULT_VERSION="v1.4.0"
fi
VERSION="${1:-${DEFAULT_VERSION}}"
TIMESTAMP=$(date '+%Y%m%d-%H%M%S')
OUTPUT_DIR="${ROOT_DIR}/build"
PACKAGE_NAME="ocr-server-production-${VERSION}-${TIMESTAMP}.tar.gz"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TMP_DIR}"' EXIT

NATS_IMAGE="${NATS_IMAGE:-nats:2.10-alpine}"

IMAGES_DIR="${TMP_DIR}/images"
CONFIG_DIR="${TMP_DIR}/config"
CERTS_DIR="${TMP_DIR}/certs"
SCRIPTS_DIR="${TMP_DIR}/scripts"
DOCS_DIR="${TMP_DIR}/docs"
PACKAGES_DIR="${TMP_DIR}/packages"

mkdir -p "${IMAGES_DIR}" "${CONFIG_DIR}" "${CERTS_DIR}" "${SCRIPTS_DIR}" "${DOCS_DIR}" "${PACKAGES_DIR}" "${OUTPUT_DIR}"

echo "=========================================="
echo "OCR系统 - 生产环境离线部署包生成"
echo "版本: ${VERSION}"
echo "时间: ${TIMESTAMP}"
echo "输出: ${OUTPUT_DIR}/${PACKAGE_NAME}"
echo "=========================================="

# 1. 导出 Docker 镜像
log_info "导出 Docker 镜像"
for image in ocr-server:latest "${NATS_IMAGE}"; do
  IMAGE_ID=$(docker image ls -q "${image}" 2>/dev/null || true)
  if [[ -z "${IMAGE_ID}" ]]; then
    log_warn "镜像 ${image} 未找到，跳过"
    continue
  fi
  FILE_NAME="${image%%:*}.tar"
  docker save "${image}" -o "${IMAGES_DIR}/${FILE_NAME}"
  SIZE=$(du -h "${IMAGES_DIR}/${FILE_NAME}" | cut -f1)
  log_ok "导出 ${image} => ${FILE_NAME} (${SIZE})"
done

# 2. 复制配置模板（纯 YAML，无 .env 依赖）
log_info "复制配置模板"
copy_config() {
  local src=$1
  local dest=$2
  if [[ -f "${src}" ]]; then
    cp "${src}" "${dest}"
    log_ok "复制 $(basename "${src}")"
  else
    log_warn "缺少配置文件: ${src}"
  fi
}

copy_config "${ROOT_DIR}/config/config.template.yaml" "${CONFIG_DIR}/config.template.yaml"

# 3. 证书
log_info "处理 TLS 证书"
if [[ -f "${ROOT_DIR}/certs/nats/ca-cert.pem" ]]; then
  cp -R "${ROOT_DIR}/certs/nats" "${CERTS_DIR}/"
  log_ok "已包含现有 TLS 证书"
else
  mkdir -p "${CERTS_DIR}/nats"
  cat > "${CERTS_DIR}/README.txt" <<'CERT'
未检测到本地 NATS TLS 证书。
部署前请在目标服务器执行：
  ./scripts/generate-nats-certs.sh certs/nats "ocr-nats,localhost,127.0.0.1"
CERT
  log_warn "未找到 TLS 证书，已添加生成说明"
fi

# 4. 部署脚本
log_info "复制部署脚本"
SCRIPT_LIST=(
  deploy-production.sh
  validate-production-env.sh
  cluster-manager.sh
  start-worker.sh
  restart-distributed.sh
  dm-gateway.sh
  generate-nats-certs.sh
)
for script in "${SCRIPT_LIST[@]}"; do
  if [[ -f "${ROOT_DIR}/scripts/${script}" ]]; then
    cp "${ROOT_DIR}/scripts/${script}" "${SCRIPTS_DIR}/"
    chmod +x "${SCRIPTS_DIR}/${script}"
    log_ok "复制 ${script}"
  else
    log_warn "缺少脚本: ${script}"
  fi
done

# 5. 文档
log_info "复制文档"
DOC_LIST=(
  PRODUCTION_DEPLOYMENT_GUIDE.md
  QUICK_REFERENCE.md
  NETWORK_AND_PORTS.md
  CONFIGURATION.md
  DISTRIBUTED_DEPLOYMENT.md
  API.md
)
for doc in "${DOC_LIST[@]}"; do
  if [[ -f "${ROOT_DIR}/docs/${doc}" ]]; then
    cp "${ROOT_DIR}/docs/${doc}" "${DOCS_DIR}/"
    log_ok "复制 ${doc}"
  else
    log_warn "缺少文档: ${doc}"
  fi
done

[[ -f "${ROOT_DIR}/README.md" ]] && cp "${ROOT_DIR}/README.md" "${TMP_DIR}/"
[[ -f "${ROOT_DIR}/DEPLOYMENT_SUMMARY.md" ]] && cp "${ROOT_DIR}/DEPLOYMENT_SUMMARY.md" "${TMP_DIR}/"
[[ -f "${ROOT_DIR}/VERSION" ]] && cp "${ROOT_DIR}/VERSION" "${TMP_DIR}/VERSION"

# 7. 导出系统依赖，便于离线环境安装
APT_DEPENDENCIES=(
  wkhtmltopdf
  libxrender1
  libxext6
  libfontconfig1
  libjpeg62-turbo
  xfonts-75dpi
  xfonts-base
  fonts-wqy-zenhei
  fonts-wqy-microhei
  libreoffice
  libreoffice-java-common
)

if command -v apt-get >/dev/null 2>&1; then
  log_info "导出 wkhtmltopdf / LibreOffice 等依赖包"
  pushd "${PACKAGES_DIR}" >/dev/null
  for pkg in "${APT_DEPENDENCIES[@]}"; do
    if apt-cache show "${pkg}" >/dev/null 2>&1; then
      if apt-get download "${pkg}" >/dev/null 2>&1; then
        log_ok "下载依赖包 ${pkg}"
      else
        log_warn "下载依赖包失败: ${pkg}（请手动下载）"
      fi
    else
      log_warn "apt-cache 中未找到依赖包: ${pkg}"
    fi
  done
  popd >/dev/null
else
  log_warn "未检测到 apt-get，跳过依赖包导出；请在离线环境启动前手动安装 wkhtmltopdf 和 LibreOffice"
fi

# 6. 生成 INSTALL.md 与 quick-deploy.sh
log_info "生成部署说明"
cat > "${TMP_DIR}/INSTALL.md" <<'INSTALL'
# OCR智能预审系统 - 离线部署指南

## 📦 包含内容

```
images/                 # Docker 镜像 (ocr-server.tar, nats.tar)
config/                 # YAML 配置模板（Master / Worker）
certs/                  # TLS 证书或生成说明
scripts/                # 部署与运维脚本
docs/                   # 生产部署相关文档
INSTALL.md              # 本指南
quick-deploy.sh         # 单机快速部署脚本
README.md, ...          # 其他参考文件
```

## 🚀 快速开始

1. **加载镜像**
   ```bash
   cd images
   docker load -i ocr-server.tar
   docker load -i nats.tar
   cd ..
   ```
2. **准备配置**
   ```bash
   mkdir -p config/master runtime/master
   cp config/config.template.yaml config/master/config.yaml
   # 按环境修改 YAML，填写数据库、OSS、Worker 凭证等敏感信息
   vim config/master/config.yaml
   ```
3. **启动 Master + NATS**
   ```bash
   ./scripts/deploy-production.sh master
   ```
4. **验证**
   ```bash
   curl http://localhost:8964/api/health
   ```
5. **部署 Worker**（如需要）
   ```bash
   ./scripts/start-worker.sh worker-01
   ```

详细说明请参阅 `docs/PRODUCTION_DEPLOYMENT_GUIDE.md`。
INSTALL

cat > "${TMP_DIR}/quick-deploy.sh" <<'QD'
#!/usr/bin/env bash
set -euo pipefail

BASE_DIR="${1:-$(pwd)}"
MASTER_DIR="${BASE_DIR}/config/master"
RUNTIME_DIR="${BASE_DIR}/runtime/master"
CERT_DIR="${BASE_DIR}/certs/nats"

mkdir -p "${MASTER_DIR}" "${RUNTIME_DIR}" "${CERT_DIR}"

if [[ ! -f "${MASTER_DIR}/config.yaml" ]]; then
  cp "config/config.template.yaml" "${MASTER_DIR}/config.yaml"
  echo "[INFO] 已生成 ${MASTER_DIR}/config.yaml，请按实际环境修改（设置角色/分布式开关等）"
fi

if [[ ! -f "${CERT_DIR}/server-cert.pem" ]]; then
  echo "[WARN] 未检测到 TLS 证书，可运行 ./scripts/generate-nats-certs.sh certs/nats \"ocr-nats,localhost,127.0.0.1\""
fi

./scripts/deploy-production.sh master
QD
chmod +x "${TMP_DIR}/quick-deploy.sh"

# 7. 打包
log_info "生成压缩包"
tar -czf "${OUTPUT_DIR}/${PACKAGE_NAME}" -C "${TMP_DIR}" .
log_ok "离线包已生成: ${OUTPUT_DIR}/${PACKAGE_NAME}"

echo "\n包内文件预览:"
tar -tzf "${OUTPUT_DIR}/${PACKAGE_NAME}" | head -n 30
