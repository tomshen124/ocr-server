#!/bin/bash
# 快速API查看脚本
# 用法: ./quick-api.sh [命令] [参数]

SERVER="${OCR_SERVER:-http://localhost:8964}"
SESSION="${OCR_SESSION:-your-session-id-here}"

# 颜色
G='\033[0;32m'; Y='\033[1;33m'; C='\033[0;36m'; NC='\033[0m'

api() {
  local url="$SERVER$1"
  [[ "$url" == *"?"* ]] && url="${url}&monitor_session_id=$SESSION" || url="${url}?monitor_session_id=$SESSION"
  curl -sk "$url" | jq . 2>/dev/null || curl -sk "$url"
}
api_open() { curl -sk "$SERVER$1" | jq . 2>/dev/null || curl -sk "$SERVER$1"; }

case "${1:-help}" in
  # 公开接口
  health)     api_open "/api/health" ;;
  details)    api_open "/api/health/details" ;;
  queue)      api_open "/api/queue/status" ;;
  stats)      api_open "/api/stats/calls" ;;
  worker)     api_open "/api/dynamic-worker/status" ;;
  failover)   api_open "/api/failover/status" ;;

  # 需要session的接口
  data)       api "/api/preview/data/${2:?需要preview_id}" ;;
  status)     api "/api/preview/status/${2:?需要preview_id}" ;;
  result)     api "/api/preview/result/${2:?需要preview_id}" ;;
  records)    api "/api/preview/records?limit=${2:-20}" ;;
  requests)   api "/api/preview/requests?limit=${2:-20}" ;;
  statistics) api "/api/preview/statistics" ;;
  failures)   api "/api/preview/failures?limit=${2:-10}" ;;
  monitoring) api "/api/monitoring/status" ;;
  logs)       api "/api/logs/stats" ;;
  rules)      api "/api/rules/matters" ;;

  # 查看刚才的预审
  last)
    PREVIEW_ID=$(tail -1 /tmp/stress_test_success.log 2>/dev/null | sed -n 's/.*previewId=\([^ ]*\).*/\1/p')
    [ -z "$PREVIEW_ID" ] && { echo "没有找到最近的预审ID"; exit 1; }
    echo -e "${G}最近预审ID:${NC} $PREVIEW_ID"
    api "/api/preview/data/$PREVIEW_ID"
    ;;

  help|*)
    echo -e "${C}OCR API 快速查看工具${NC}"
    echo -e "用法: $0 <命令> [参数]\n"
    echo -e "${Y}公开接口:${NC}"
    echo "  health      - 健康检查"
    echo "  details     - 详细健康"
    echo "  queue       - 队列状态"
    echo "  stats       - 调用统计"
    echo "  worker      - Worker状态"
    echo "  failover    - 故障转移状态"
    echo -e "\n${Y}需要session的接口:${NC}"
    echo "  data <id>   - 预审数据"
    echo "  status <id> - 预审状态"
    echo "  result <id> - 预审结果"
    echo "  records [n] - 预审记录(默认20条)"
    echo "  requests [n]- 请求列表"
    echo "  statistics  - 预审统计"
    echo "  failures [n]- 失败记录"
    echo "  monitoring  - 系统状态"
    echo "  logs        - 日志统计"
    echo "  rules       - 规则列表"
    echo "  last        - 查看最近一次预审"
    echo -e "\n${Y}环境变量:${NC}"
    echo "  OCR_SERVER  - 服务器地址 (当前: $SERVER)"
    echo "  OCR_SESSION - Session ID (当前: ${SESSION:0:8}...)"
    ;;
esac
