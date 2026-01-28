#!/usr/bin/env node

const fs = require('fs-extra');
const path = require('path');
const { minify } = require('terser');
const CleanCSS = require('clean-css');
const { minify: minifyHtml } = require('html-minifier-terser');
const glob = require('glob');
const chalk = require('chalk');

class FrontendBuilder {
    constructor() {
        this.sourceDir = path.resolve(__dirname, '../static');
        this.outputDir = path.resolve(__dirname, '../static-dist');
        this.isDevelopment = process.argv.includes('--dev');
        this.isProduction = process.argv.includes('--prod');
        
        // 如果没有指定环境，默认为生产环境
        if (!this.isDevelopment && !this.isProduction) {
            this.isProduction = true;
        }
        
        console.log(chalk.blue(`🏗️  构建模式: ${this.isDevelopment ? '开发环境' : '生产环境'}`));
    }

    async build() {
        try {
            await this.clean();
            await this.copyStructure();
            await this.processJavaScript();
            await this.processCSS();
            await this.processHTML();
            
            console.log(chalk.green('✅ 前端构建完成!'));
            this.printSummary();
        } catch (error) {
            console.error(chalk.red('❌ 构建失败:'), error);
            process.exit(1);
        }
    }

    async clean() {
        console.log(chalk.yellow('🧹 清理输出目录...'));
        await fs.remove(this.outputDir);
        await fs.ensureDir(this.outputDir);
    }

    async copyStructure() {
        console.log(chalk.yellow('📁 复制目录结构...'));
        
        // 复制除了js/css/html之外的所有文件
        const filesToCopy = glob.sync('**/*', {
            cwd: this.sourceDir,
            ignore: ['**/*.js', '**/*.css', '**/*.html'],
            nodir: true
        });

        for (const file of filesToCopy) {
            const sourcePath = path.join(this.sourceDir, file);
            const outputPath = path.join(this.outputDir, file);
            
            await fs.ensureDir(path.dirname(outputPath));
            await fs.copy(sourcePath, outputPath);
        }
    }

    async processJavaScript() {
        console.log(chalk.yellow('⚙️  处理JavaScript文件...'));
        
        const jsFiles = glob.sync('**/*.js', { cwd: this.sourceDir });
        
        for (const jsFile of jsFiles) {
            const sourcePath = path.join(this.sourceDir, jsFile);
            const outputPath = path.join(this.outputDir, jsFile);
            
            await fs.ensureDir(path.dirname(outputPath));
            
            const sourceCode = await fs.readFile(sourcePath, 'utf8');
            
            if (this.isProduction) {
                // 生产环境：混淆和压缩
                const result = await minify(sourceCode, {
                    compress: {
                        drop_console: true,      // 移除console语句
                        drop_debugger: true,     // 移除debugger语句
                        pure_funcs: ['console.log', 'console.info', 'console.debug'],
                        passes: 2
                    },
                    mangle: {
                        toplevel: true,          // 混淆顶级作用域
                        properties: {
                            regex: /^_/          // 混淆以_开头的属性
                        }
                    },
                    format: {
                        comments: false          // 移除注释
                    },
                    sourceMap: false             // 不生成source map
                });
                
                await fs.writeFile(outputPath, result.code);
                console.log(chalk.green(`  ✓ ${jsFile} (混淆压缩)`));
            } else {
                // 开发环境：仅复制，保留源码
                await fs.writeFile(outputPath, sourceCode);
                console.log(chalk.cyan(`  ✓ ${jsFile} (源码保留)`));
            }
        }
    }

    async processCSS() {
        console.log(chalk.yellow('🎨 处理CSS文件...'));
        
        const cssFiles = glob.sync('**/*.css', { cwd: this.sourceDir });
        const cleanCSS = new CleanCSS({
            level: this.isProduction ? 2 : 0  // 生产环境高级优化
        });
        
        for (const cssFile of cssFiles) {
            const sourcePath = path.join(this.sourceDir, cssFile);
            const outputPath = path.join(this.outputDir, cssFile);
            
            await fs.ensureDir(path.dirname(outputPath));
            
            const sourceCode = await fs.readFile(sourcePath, 'utf8');
            
            if (this.isProduction) {
                const result = cleanCSS.minify(sourceCode);
                await fs.writeFile(outputPath, result.styles);
                console.log(chalk.green(`  ✓ ${cssFile} (压缩)`));
            } else {
                await fs.writeFile(outputPath, sourceCode);
                console.log(chalk.cyan(`  ✓ ${cssFile} (源码保留)`));
            }
        }
    }

    async processHTML() {
        console.log(chalk.yellow('📄 处理HTML文件...'));
        
        const htmlFiles = glob.sync('**/*.html', { cwd: this.sourceDir });
        
        for (const htmlFile of htmlFiles) {
            const sourcePath = path.join(this.sourceDir, htmlFile);
            const outputPath = path.join(this.outputDir, htmlFile);
            
            await fs.ensureDir(path.dirname(outputPath));
            
            const sourceCode = await fs.readFile(sourcePath, 'utf8');
            
            if (this.isProduction) {
                const result = await minifyHtml(sourceCode, {
                    collapseWhitespace: true,
                    removeComments: true,
                    removeRedundantAttributes: true,
                    removeScriptTypeAttributes: true,
                    removeStyleLinkTypeAttributes: true,
                    minifyCSS: true,
                    minifyJS: true
                });
                
                await fs.writeFile(outputPath, result);
                console.log(chalk.green(`  ✓ ${htmlFile} (压缩)`));
            } else {
                await fs.writeFile(outputPath, sourceCode);
                console.log(chalk.cyan(`  ✓ ${htmlFile} (源码保留)`));
            }
        }
    }

    async printSummary() {
        const sourceSize = await this.getDirectorySize(this.sourceDir);
        const outputSize = await this.getDirectorySize(this.outputDir);
        const compression = ((sourceSize - outputSize) / sourceSize * 100).toFixed(1);
        
        console.log(chalk.blue('\n📊 构建统计:'));
        console.log(`  源码大小: ${this.formatBytes(sourceSize)}`);
        console.log(`  输出大小: ${this.formatBytes(outputSize)}`);
        
        if (this.isProduction) {
            console.log(`  压缩率: ${compression}%`);
            console.log(chalk.green(`  🎉 生产代码已混淆压缩!`));
        } else {
            console.log(chalk.cyan(`  🔧 开发版本已构建!`));
        }
    }

    async getDirectorySize(dirPath) {
        const files = await fs.readdir(dirPath, { withFileTypes: true });
        let size = 0;
        
        for (const file of files) {
            const filePath = path.join(dirPath, file.name);
            if (file.isDirectory()) {
                size += await this.getDirectorySize(filePath);
            } else {
                const stats = await fs.stat(filePath);
                size += stats.size;
            }
        }
        
        return size;
    }

    formatBytes(bytes) {
        if (bytes === 0) return '0 Bytes';
        const k = 1024;
        const sizes = ['Bytes', 'KB', 'MB', 'GB'];
        const i = Math.floor(Math.log(bytes) / Math.log(k));
        return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i];
    }
}

// 执行构建
const builder = new FrontendBuilder();
builder.build();