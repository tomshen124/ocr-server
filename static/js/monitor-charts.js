/**
 * 监控系统图表增强模块
 * 提供实时数据图表展示功能
 */

(function() {
    'use strict';

    // 图表实例存储
    window.MonitorCharts = {
        resourceTrend: null,
        loadTrend: null,
        statusDistribution: null,
        durationTrend: null
    };

    // 数据历史存储
    const dataHistory = {
        cpu: [],
        memory: [],
        disk: [],
        timestamps: [],
        durations: []
    };

    const MAX_DATA_POINTS = 20;

    // Chart.js 默认配置
    const chartDefaults = {
        responsive: true,
        maintainAspectRatio: false,
        plugins: {
            legend: {
                labels: {
                    font: {
                        family: '-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto',
                        size: 12
                    },
                    color: '#5c5c5c',
                    padding: 12,
                    usePointStyle: true
                }
            },
            tooltip: {
                backgroundColor: 'rgba(0, 0, 0, 0.8)',
                padding: 12,
                titleFont: {
                    size: 13,
                    weight: '600'
                },
                bodyFont: {
                    size: 12
                },
                borderColor: 'rgba(22, 119, 255, 0.3)',
                borderWidth: 1
            }
        },
        interaction: {
            mode: 'index',
            intersect: false
        }
    };

    /**
     * 初始化资源趋势图表 (CPU & 内存)
     */
    function initResourceTrendChart() {
        const canvas = document.getElementById('resourceTrendChart');
        if (!canvas) return;

        const ctx = canvas.getContext('2d');

        window.MonitorCharts.resourceTrend = new Chart(ctx, {
            type: 'line',
            data: {
                labels: [],
                datasets: [
                    {
                        label: 'CPU 使用率',
                        data: [],
                        borderColor: '#1677ff',
                        backgroundColor: 'rgba(22, 119, 255, 0.1)',
                        borderWidth: 2,
                        fill: true,
                        tension: 0.4,
                        pointRadius: 3,
                        pointBackgroundColor: '#1677ff',
                        pointBorderColor: '#fff',
                        pointBorderWidth: 2,
                        pointHoverRadius: 5
                    },
                    {
                        label: '内存 使用率',
                        data: [],
                        borderColor: '#52c41a',
                        backgroundColor: 'rgba(82, 196, 26, 0.1)',
                        borderWidth: 2,
                        fill: true,
                        tension: 0.4,
                        pointRadius: 3,
                        pointBackgroundColor: '#52c41a',
                        pointBorderColor: '#fff',
                        pointBorderWidth: 2,
                        pointHoverRadius: 5
                    }
                ]
            },
            options: {
                ...chartDefaults,
                scales: {
                    x: {
                        grid: {
                            color: 'rgba(0, 0, 0, 0.05)',
                            drawBorder: false
                        },
                        ticks: {
                            color: '#8f8f8f',
                            font: {
                                size: 11
                            },
                            maxRotation: 0
                        }
                    },
                    y: {
                        min: 0,
                        max: 100,
                        grid: {
                            color: 'rgba(0, 0, 0, 0.05)',
                            drawBorder: false
                        },
                        ticks: {
                            color: '#8f8f8f',
                            font: {
                                size: 11
                            },
                            callback: function(value) {
                                return value + '%';
                            }
                        }
                    }
                },
                plugins: {
                    ...chartDefaults.plugins,
                    tooltip: {
                        ...chartDefaults.plugins.tooltip,
                        callbacks: {
                            label: function(context) {
                                return context.dataset.label + ': ' + context.parsed.y.toFixed(1) + '%';
                            }
                        }
                    }
                }
            }
        });

        console.log('✓ 资源趋势图表初始化成功');
    }

    /**
     * 初始化系统负载趋势图���
     */
    function initLoadTrendChart() {
        const canvas = document.getElementById('loadTrendChart');
        if (!canvas) return;

        const ctx = canvas.getContext('2d');

        window.MonitorCharts.loadTrend = new Chart(ctx, {
            type: 'line',
            data: {
                labels: [],
                datasets: [
                    {
                        label: '磁盘 I/O',
                        data: [],
                        borderColor: '#faad14',
                        backgroundColor: 'rgba(250, 173, 20, 0.1)',
                        borderWidth: 2,
                        fill: true,
                        tension: 0.4,
                        pointRadius: 3,
                        pointBackgroundColor: '#faad14',
                        pointBorderColor: '#fff',
                        pointBorderWidth: 2,
                        pointHoverRadius: 5
                    }
                ]
            },
            options: {
                ...chartDefaults,
                scales: {
                    x: {
                        grid: {
                            color: 'rgba(0, 0, 0, 0.05)',
                            drawBorder: false
                        },
                        ticks: {
                            color: '#8f8f8f',
                            font: {
                                size: 11
                            },
                            maxRotation: 0
                        }
                    },
                    y: {
                        min: 0,
                        max: 100,
                        grid: {
                            color: 'rgba(0, 0, 0, 0.05)',
                            drawBorder: false
                        },
                        ticks: {
                            color: '#8f8f8f',
                            font: {
                                size: 11
                            },
                            callback: function(value) {
                                return value + '%';
                            }
                        }
                    }
                }
            }
        });

        console.log('✓ 系统负载图表初始化成功');
    }

    /**
     * 初始化状态分布饼图
     */
    function initStatusDistributionChart() {
        const canvas = document.getElementById('statusDistributionChart');
        if (!canvas) return;

        const ctx = canvas.getContext('2d');

        window.MonitorCharts.statusDistribution = new Chart(ctx, {
            type: 'doughnut',
            data: {
                labels: ['已完成', '处理中', '失败', '排队中'],
                datasets: [{
                    data: [0, 0, 0, 0],
                    backgroundColor: [
                        'rgba(82, 196, 26, 0.8)',    // 绿色 - 已完成
                        'rgba(22, 119, 255, 0.8)',   // 蓝色 - 处理中
                        'rgba(255, 77, 79, 0.8)',     // 红色 - 失败
                        'rgba(250, 173, 20, 0.8)'     // 橙色 - 排队中
                    ],
                    borderColor: '#ffffff',
                    borderWidth: 2,
                    hoverOffset: 10
                }]
            },
            options: {
                ...chartDefaults,
                cutout: '65%',
                plugins: {
                    ...chartDefaults.plugins,
                    legend: {
                        ...chartDefaults.plugins.legend,
                        position: 'bottom',
                        labels: {
                            ...chartDefaults.plugins.legend.labels,
                            padding: 16,
                            generateLabels: function(chart) {
                                const data = chart.data;
                                if (data.labels.length && data.datasets.length) {
                                    return data.labels.map((label, i) => {
                                        const value = data.datasets[0].data[i];
                                        const total = data.datasets[0].data.reduce((a, b) => a + b, 0);
                                        const percentage = total > 0 ? ((value / total) * 100).toFixed(1) : 0;

                                        return {
                                            text: `${label}: ${value} (${percentage}%)`,
                                            fillStyle: data.datasets[0].backgroundColor[i],
                                            hidden: false,
                                            index: i
                                        };
                                    });
                                }
                                return [];
                            }
                        }
                    },
                    tooltip: {
                        ...chartDefaults.plugins.tooltip,
                        callbacks: {
                            label: function(context) {
                                const label = context.label || '';
                                const value = context.parsed;
                                const total = context.dataset.data.reduce((a, b) => a + b, 0);
                                const percentage = ((value / total) * 100).toFixed(1);
                                return `${label}: ${value} (${percentage}%)`;
                            }
                        }
                    }
                }
            }
        });

        console.log('✓ 状态分布图表初始化成功');
    }

    /**
     * 初始化处理时长趋势图表
     */
    function initDurationTrendChart() {
        const canvas = document.getElementById('durationTrendChart');
        if (!canvas) return;

        const ctx = canvas.getContext('2d');

        window.MonitorCharts.durationTrend = new Chart(ctx, {
            type: 'bar',
            data: {
                labels: [],
                datasets: [{
                    label: '平均处理时长 (秒)',
                    data: [],
                    backgroundColor: 'rgba(22, 119, 255, 0.6)',
                    borderColor: '#1677ff',
                    borderWidth: 1,
                    borderRadius: 4,
                    hoverBackgroundColor: 'rgba(22, 119, 255, 0.8)'
                }]
            },
            options: {
                ...chartDefaults,
                scales: {
                    x: {
                        grid: {
                            display: false,
                            drawBorder: false
                        },
                        ticks: {
                            color: '#8f8f8f',
                            font: {
                                size: 11
                            }
                        }
                    },
                    y: {
                        beginAtZero: true,
                        grid: {
                            color: 'rgba(0, 0, 0, 0.05)',
                            drawBorder: false
                        },
                        ticks: {
                            color: '#8f8f8f',
                            font: {
                                size: 11
                            },
                            callback: function(value) {
                                return value + 's';
                            }
                        }
                    }
                },
                plugins: {
                    ...chartDefaults.plugins,
                    tooltip: {
                        ...chartDefaults.plugins.tooltip,
                        callbacks: {
                            label: function(context) {
                                return '平均时长: ' + context.parsed.y.toFixed(2) + '秒';
                            }
                        }
                    }
                }
            }
        });

        console.log('✓ 处理时长趋势图表初始化成功');
    }

    /**
     * 更新资源趋势图表数据
     */
    function updateResourceTrendChart(cpuUsage, memoryUsage, diskUsage) {
        if (!window.MonitorCharts.resourceTrend) return;

        const now = new Date();
        const timeLabel = now.toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit', second: '2-digit' });

        dataHistory.timestamps.push(timeLabel);
        dataHistory.cpu.push(cpuUsage);
        dataHistory.memory.push(memoryUsage);
        dataHistory.disk.push(diskUsage);

        // 保持最多20个数据点
        if (dataHistory.timestamps.length > MAX_DATA_POINTS) {
            dataHistory.timestamps.shift();
            dataHistory.cpu.shift();
            dataHistory.memory.shift();
            dataHistory.disk.shift();
        }

        const chart = window.MonitorCharts.resourceTrend;
        chart.data.labels = [...dataHistory.timestamps];
        chart.data.datasets[0].data = [...dataHistory.cpu];
        chart.data.datasets[1].data = [...dataHistory.memory];
        chart.update('none'); // 使用'none'模式避免动画，提高性能

        // 更新负载趋势图
        if (window.MonitorCharts.loadTrend) {
            window.MonitorCharts.loadTrend.data.labels = [...dataHistory.timestamps];
            window.MonitorCharts.loadTrend.data.datasets[0].data = [...dataHistory.disk];
            window.MonitorCharts.loadTrend.update('none');
        }
    }

    /**
     * 更新状态分布图表数据
     */
    function updateStatusDistributionChart(completed, processing, failed, queued) {
        if (!window.MonitorCharts.statusDistribution) return;

        const chart = window.MonitorCharts.statusDistribution;
        chart.data.datasets[0].data = [completed, processing, failed, queued];
        chart.update();
    }

    /**
     * 更新处理时长趋势图表
     */
    function updateDurationTrendChart(durations) {
        if (!window.MonitorCharts.durationTrend) return;

        const chart = window.MonitorCharts.durationTrend;

        // 提取最近10条数据
        const recentDurations = durations.slice(-10);
        const labels = recentDurations.map((_, index) => `#${index + 1}`);
        const data = recentDurations.map(d => d / 1000); // 转换为秒

        chart.data.labels = labels;
        chart.data.datasets[0].data = data;
        chart.update();
    }

    /**
     * 初始化所有图表
     */
    function initAllCharts() {
        // 等待Chart.js加载完成
        if (typeof Chart === 'undefined') {
            console.warn('Chart.js未加载，延迟初始化图表');
            setTimeout(initAllCharts, 500);
            return;
        }

        console.log('开始初始化监控图表...');

        initResourceTrendChart();
        initLoadTrendChart();
        initStatusDistributionChart();
        initDurationTrendChart();

        console.log('✓ 所有监控图表初始化完成');
    }

    // 暴露公共方法
    window.MonitorCharts.init = initAllCharts;
    window.MonitorCharts.updateResourceTrend = updateResourceTrendChart;
    window.MonitorCharts.updateStatusDistribution = updateStatusDistributionChart;
    window.MonitorCharts.updateDurationTrend = updateDurationTrendChart;

    // DOM加载完成后自动初始化
    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', initAllCharts);
    } else {
        initAllCharts();
    }

})();
