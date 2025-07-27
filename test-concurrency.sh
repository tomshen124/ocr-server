#!/bin/bash

# OCR并发控制测试脚本
# 验证32核64G服务器的并发限制是否正常工作

echo "🚀 开始OCR并发控制测试"
echo "================================================"

# 检查服务是否运行
echo "1. 检查OCR服务状态..."
if ! curl -s http://localhost:31101/api/health > /dev/null; then
    echo "❌ OCR服务未运行，请先启动服务"
    exit 1
fi
echo "✅ OCR服务运行正常"

# 检查队列状态API
echo ""
echo "2. 检查队列状态API..."
QUEUE_STATUS=$(curl -s http://localhost:31101/api/queue/status)
if [ $? -eq 0 ]; then
    echo "✅ 队列状态API响应正常"
    echo "当前队列状态:"
    echo "$QUEUE_STATUS" | jq '.data.queue' 2>/dev/null || echo "$QUEUE_STATUS"
else
    echo "❌ 队列状态API无响应"
    exit 1
fi

# 模拟并发请求测试（如果存在测试数据）
echo ""
echo "3. 模拟并发测试..."

# 检查是否有测试数据文件
if [ -f "qingqiu.json" ]; then
    echo "找到测试数据文件，开始并发测试..."
    
    # 启动15个并发请求（超过12的限制）
    echo "启动15个并发OCR请求（超过12个限制，测试排队机制）..."
    
    for i in {1..15}; do
        {
            echo "启动请求 #$i"
            RESPONSE=$(curl -s -X POST http://localhost:31101/api/preview \
                -H "Content-Type: application/json" \
                -d @qingqiu.json)
            
            if echo "$RESPONSE" | grep -q '"success":true'; then
                echo "✅ 请求 #$i 提交成功"
            else
                echo "❌ 请求 #$i 提交失败: $RESPONSE"
            fi
        } &
        
        # 稍微错开请求时间
        sleep 0.1
    done
    
    echo "等待所有请求完成..."
    wait
    
    # 检查最终队列状态
    echo ""
    echo "4. 检查处理后的队列状态..."
    sleep 2
    FINAL_STATUS=$(curl -s http://localhost:31101/api/queue/status)
    echo "最终队列状态:"
    echo "$FINAL_STATUS" | jq '.data' 2>/dev/null || echo "$FINAL_STATUS"
    
else
    echo "⚠️ 未找到qingqiu.json测试数据文件，跳过并发测试"
    echo "如需测试，请准备测试数据文件"
fi

echo ""
echo "5. 监控系统资源使用..."
echo "内存使用情况:"
free -h | head -2

echo ""
echo "CPU负载:"
top -bn1 | grep "load average" || uptime

echo ""
echo "OCR服务进程状态:"
ps aux | grep ocr-server | grep -v grep || echo "未找到ocr-server进程"

echo ""
echo "================================================"
echo "🎯 并发控制测试完成"
echo ""
echo "📊 关键指标检查:"
echo "- 最大并发限制: 12个OCR任务"
echo "- 服务器配置: 32核64G"
echo "- 预期行为: 超过12个任务时应该排队等待"
echo ""
echo "🔧 如需调整并发限制，请修改 src/main.rs 中的 max_concurrent 值"
echo "💡 监控队列状态: curl http://localhost:31101/api/queue/status"