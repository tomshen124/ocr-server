use chrono::Utc;
use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Command,
    sync::LazyLock,
};

static OUT_DIR: LazyLock<PathBuf> = LazyLock::new(|| {
    let out_dir = std::env::var("OUT_DIR").unwrap();
    Path::new(&out_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
});

fn main() -> io::Result<()> {
    println!("cargo:rerun-if-changed=VERSION");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/heads");

    set_build_metadata();

    // 监听文件变化
    println!("cargo:rerun-if-changed=static");
    println!("cargo:rerun-if-changed=root");
    println!("cargo:rerun-if-changed=ocr");
    println!("cargo:rerun-if-changed=config.yaml");
    println!("cargo:rerun-if-changed=config.example.yaml");
    println!("cargo:rerun-if-changed=src/monitor");
    println!("cargo:rerun-if-changed=ocr-server.sh");
    println!("cargo:rerun-if-changed=ocr-monitor.sh");
    println!("cargo:rerun-if-changed=service-manager.sh");

    println!("cargo:info=开始构建OCR服务...");

    // 复制OCR引擎文件
    if Path::new("ocr").exists() {
        copy_dir_all("ocr", OUT_DIR.join("ocr"))?;
        println!("cargo:info=已复制OCR引擎文件");
    } else {
        println!("cargo:warning=OCR引擎目录不存在，跳过复制");
    }

    // 复制前端静态文件（包含新的监控页面）
    if Path::new("static").exists() {
        copy_dir_all("static", OUT_DIR.join("static"))?;
        println!("cargo:info=已复制前端静态文件（包含监控页面）");
    } else {
        println!("cargo:warning=静态文件目录不存在");
    }

    // 复制根目录文件
    if Path::new("root").exists() {
        copy_dir_all("root", OUT_DIR.join("root"))?;
        println!("cargo:info=已复制根目录文件");
    }

    // 复制配置文件
    copy_config_files()?;

    // 复制服务管理脚本
    copy_service_scripts()?;

    // 复制文档文件
    copy_documentation()?;

    // 创建必要的运行时目录
    create_runtime_directories()?;

    println!(
        "cargo:info=构建完成！所有部署文件已复制到: {}",
        OUT_DIR.display()
    );

    Ok(())
}

fn set_build_metadata() {
    let git_commit = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string());

    let build_version = fs::read_to_string("VERSION")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| git_commit.clone());

    let build_timestamp = Utc::now().to_rfc3339();

    println!("cargo:rustc-env=APP_BUILD_VERSION={}", build_version);
    println!("cargo:rustc-env=APP_BUILD_COMMIT={}", git_commit);
    println!("cargo:rustc-env=APP_BUILD_TIMESTAMP={}", build_timestamp);
}

// 复制配置文件
fn copy_config_files() -> io::Result<()> {
    // 复制主配置文件
    if Path::new("config.yaml").exists() {
        fs::copy("config.yaml", OUT_DIR.join("config.yaml"))?;
        println!("cargo:info=已复制主配置文件");
    }

    // 复制示例配置文件
    if Path::new("config.example.yaml").exists() {
        fs::copy("config.example.yaml", OUT_DIR.join("config.example.yaml"))?;
        println!("cargo:info=已复制示例配置文件");
    }

    Ok(())
}

// 复制服务管理脚本
fn copy_service_scripts() -> io::Result<()> {
    let scripts = ["ocr-server.sh", "ocr-monitor.sh", "service-manager.sh"];

    for script in &scripts {
        if Path::new(script).exists() {
            fs::copy(script, OUT_DIR.join(script))?;
            println!("cargo:info=已复制服务脚本: {}", script);
        }
    }

    Ok(())
}

// 复制文档文件
fn copy_documentation() -> io::Result<()> {
    let docs = [
        "README.md",
        "SERVICE_ARCHITECTURE.md",
        "USAGE_GUIDE.md",
        "MONITORING_INTEGRATION.md",
        "OCR服务API文档.md",
    ];

    for doc in &docs {
        if Path::new(doc).exists() {
            fs::copy(doc, OUT_DIR.join(doc))?;
            println!("cargo:info=已复制文档: {}", doc);
        }
    }

    Ok(())
}

// 创建运行时目录
fn create_runtime_directories() -> io::Result<()> {
    let dirs = ["logs", "images", "preview", "cache", "temp"];

    for dir in &dirs {
        fs::create_dir_all(OUT_DIR.join(dir))?;
    }

    println!("cargo:info=已创建运行时目录");
    Ok(())
}

fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> io::Result<()> {
    let src = src.as_ref();
    let dst = dst.as_ref();

    if !src.exists() {
        println!("cargo:warning=源目录 {} 不存在，跳过复制", src.display());
        return Ok(());
    }

    fs::create_dir_all(&dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(entry.path(), dst.join(entry.file_name()))?;
        } else {
            let to = dst.join(entry.file_name());
            // 总是复制文件，覆盖已存在的文件
            fs::copy(entry.path(), to)?;
        }
    }
    Ok(())
}
