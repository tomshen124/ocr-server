#!/usr/bin/env bash
#
# Build the OCR server inside a CentOS7-compatible Docker container
# Usage:
#   ./scripts/build-with-docker.sh -- [build.sh args]
# Example:
#   ./scripts/build-with-docker.sh -- --prod-native
#   ./scripts/build-with-docker.sh -- -m release -t musl -p
#   ./scripts/build-with-docker.sh shell   # drop into container shell

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
DEFAULT_DOCKERFILE="$PROJECT_ROOT/docker/Dockerfile.centos7"
DOCKER_CONTEXT="${OCR_DOCKER_CONTEXT:-$PROJECT_ROOT}"
DOCKERFILE="${OCR_DOCKERFILE:-$DEFAULT_DOCKERFILE}"
IMAGE_NAME="${OCR_BUILDER_IMAGE:-ocr-server-builder:centos7}"
HOST_ARCH="$(uname -m)"

if [[ -z "${OCR_BUILDER_PLATFORM:-}" ]]; then
    if [[ "$HOST_ARCH" == "arm64" || "$HOST_ARCH" == "aarch64" ]]; then
        OCR_BUILDER_PLATFORM="linux/amd64"
    fi
fi

usage() {
    cat <<USAGE
用法: $0 [选项] -- [build.sh 参数]

选项:
  --image <name>        自定义镜像名称 (默认: $IMAGE_NAME)
  --dockerfile <path>   指定Dockerfile路径 (默认: $DOCKERFILE)
  --no-build            跳过镜像构建，直接运行容器
  shell                 进入交互式Shell，可手动编译
  -h, --help            显示本帮助

示例:
  $0 -- --prod-native
  $0 -- -m release -t musl -p -f monitoring
  $0 shell

环境变量:
  OCR_BUILDER_EXTRA_RUN_ARGS   附加传给 docker run 的参数 (如网络代理)
  OCR_BUILDER_BUILD_ARGS       附加传给 docker build 的参数 (如 --build-arg)
  OCR_BUILDER_PLATFORM         指定 Docker 平台 (默认: Mac M1 自动用 linux/amd64)
USAGE
}

DOCKER_BUILD=1
USER_ARGS=()
RUN_SHELL=0
declare -a BUILD_ARGS=()

while [[ $# -gt 0 ]]; do
    case "$1" in
        --image)
            IMAGE_NAME="$2"
            shift 2
            ;;
        --dockerfile)
            DOCKERFILE="$2"
            shift 2
            ;;
        --no-build)
            DOCKER_BUILD=0
            shift
            ;;
        shell)
            RUN_SHELL=1
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        --)
            shift
            USER_ARGS=("$@")
            break
            ;;
        *)
            echo "Unknown option: $1" >&2
            usage
            exit 1
            ;;
    esac
done

if [[ $RUN_SHELL -eq 1 && ${#USER_ARGS[@]} -gt 0 ]]; then
    echo "shell 模式不应再附带 build.sh 参数" >&2
    exit 1
fi

if [[ -n "${OCR_BUILDER_BUILD_ARGS:-}" ]]; then
    # shellcheck disable=SC2206
    BUILD_ARGS=(${OCR_BUILDER_BUILD_ARGS})
fi

if [[ -n "${http_proxy:-${HTTP_PROXY:-}}" ]]; then
    proxy_http=${http_proxy:-$HTTP_PROXY}
    proxy_https=${https_proxy:-${HTTPS_PROXY:-$proxy_http}}
    proxy_no=${no_proxy:-${NO_PROXY:-}}
    BUILD_ARGS+=("--build-arg" "http_proxy=$proxy_http" "--build-arg" "https_proxy=$proxy_https")
    if [[ -n "$proxy_no" ]]; then
        BUILD_ARGS+=("--build-arg" "no_proxy=$proxy_no")
    fi
fi

if [[ $DOCKER_BUILD -eq 1 ]]; then
    echo "[INFO] 构建Docker镜像: $IMAGE_NAME"
    if [[ -n "${OCR_BUILDER_PLATFORM:-}" ]]; then
        echo "[INFO] 使用 docker build 平台: $OCR_BUILDER_PLATFORM"
    fi
    BUILD_CMD=(docker build)
    if (( ${#BUILD_ARGS[@]} > 0 )); then
        BUILD_CMD+=("${BUILD_ARGS[@]}")
    fi
    if [[ -n "${OCR_BUILDER_PLATFORM:-}" ]]; then
        BUILD_CMD+=(--platform "$OCR_BUILDER_PLATFORM")
    fi
    BUILD_CMD+=(-t "$IMAGE_NAME" -f "$DOCKERFILE" "$DOCKER_CONTEXT")
    "${BUILD_CMD[@]}"
else
    echo "[INFO] 跳过镜像构建"
fi

RUN_ARGS=("--rm" "-t")

if [[ -n "${OCR_BUILDER_PLATFORM:-}" ]]; then
    RUN_ARGS+=("--platform" "$OCR_BUILDER_PLATFORM")
fi

# 在macOS环境下，透传当前用户UID/GID，避免产物属主为root
if command -v id >/dev/null 2>&1; then
    RUN_ARGS+=("--user" "$(id -u):$(id -g)")
fi

RUN_ARGS+=("-v" "$PROJECT_ROOT:/workspace" "-w" "/workspace")

if [[ -n "${http_proxy:-${HTTP_PROXY:-}}" ]]; then
    proxy_http=${http_proxy:-$HTTP_PROXY}
    proxy_https=${https_proxy:-${HTTPS_PROXY:-$proxy_http}}
    proxy_no=${no_proxy:-${NO_PROXY:-}}
    RUN_ARGS+=("-e" "http_proxy=$proxy_http" "-e" "https_proxy=$proxy_https" "-e" "HTTP_PROXY=$proxy_http" "-e" "HTTPS_PROXY=$proxy_https")
    if [[ -n "$proxy_no" ]]; then
        RUN_ARGS+=("-e" "no_proxy=$proxy_no" "-e" "NO_PROXY=$proxy_no")
    fi
fi

if [[ -n "${OCR_BUILDER_EXTRA_RUN_ARGS:-}" ]]; then
    # shellcheck disable=SC2206
    EXTRA_ARGS=(${OCR_BUILDER_EXTRA_RUN_ARGS})
    RUN_ARGS+=("${EXTRA_ARGS[@]}")
fi

if [[ $RUN_SHELL -eq 1 ]]; then
    if [[ -n "${OCR_BUILDER_PLATFORM:-}" ]]; then
        echo "[INFO] 使用 docker run 平台: $OCR_BUILDER_PLATFORM"
    fi
    echo "[INFO] 启动交互式容器shell"
    exec docker run -it "${RUN_ARGS[@]}" "$IMAGE_NAME" bash
fi

if [[ ${#USER_ARGS[@]} -eq 0 ]]; then
    echo "[INFO] 未提供 build.sh 参数，默认使用 --prod-native"
    USER_ARGS=("--prod-native")
fi

BUILD_CMD=("./scripts/build.sh" "${USER_ARGS[@]}")

echo "[INFO] 在容器内执行: ${BUILD_CMD[*]}"
if [[ -n "${OCR_BUILDER_PLATFORM:-}" ]]; then
    echo "[INFO] 使用 docker run 平台: $OCR_BUILDER_PLATFORM"
fi
docker run -it "${RUN_ARGS[@]}" "$IMAGE_NAME" \
    bash -lc "${BUILD_CMD[*]}"
