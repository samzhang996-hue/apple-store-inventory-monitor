//! 有货详细日志记录器。
//!
//! 在检测到 Apple 直营店有货时，以带 UTF-8 BOM 的 RFC 4180 CSV 标准格式
//! 原子追加写入到用户的文档目录下（如 `~/Documents/apple-store-inventory-monitor/有货记录.csv`）。
//! 文件可被 Microsoft Excel、WPS Office 与 Apple Numbers 原生双击打开，并自动正确分列且中文无乱码。

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::model::{PickupDetails, Target, region_by_locale};

pub const LOG_FILE_NAME: &str = "有货记录.csv";

/// 导出至 Excel 的 20 列详细字段定义
pub const HEADERS: &[&str] = &[
    "记录时间",
    "时间戳(ms)",
    "地区代码",
    "地区名称",
    "门店编号",
    "门店名称",
    "商品完整名称",
    "型号零件号",
    "搭配配件零件号",
    "搭配配件说明",
    "套件零件号",
    "库存状态",
    "取货时效描述",
    "取货时段承诺",
    "官方销售原因",
    "官方销售提示",
    "触发提醒动作",
    "查询间隔(秒)",
    "连续失败次数",
    "官网直达商品链接",
];

/// 获取日志所在目录（优先用户文档目录 ~/Documents，次选配置目录）
pub fn get_log_dir() -> PathBuf {
    dirs::document_dir()
        .map(|d| d.join("apple-store-inventory-monitor"))
        .or_else(|| dirs::config_dir().map(|c| c.join("apple-store-inventory-monitor")))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// 获取日志文件的完整路径
pub fn get_log_file_path() -> PathBuf {
    get_log_dir().join(LOG_FILE_NAME)
}

/// RFC 4180 标准 CSV 字段转义
pub fn escape_csv_field(field: &str) -> String {
    let needs_quotes = field.contains(',')
        || field.contains('"')
        || field.contains('\r')
        || field.contains('\n');
    if needs_quotes {
        let escaped = field.replace('"', "\"\"");
        format!("\"{escaped}\"")
    } else {
        field.to_string()
    }
}

/// 将基于 1970-01-01 的日偏移转换为公历 (年, 月, 日)
///
/// 采用 Howard Hinnant 日历转换算法，准确覆盖公历所有闰年与世纪闰年规则。
fn civil_from_days(days_since_1970: i64) -> (i64, u32, u32) {
    let z = days_since_1970 + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let year = if m <= 2 { y + 1 } else { y };
    (year, m, d)
}

/// 将 Unix 时间戳毫秒格式化为本地可读时间字符串 (YYYY-MM-DD HH:MM:SS)
pub fn format_epoch_ms(epoch_ms: u64, tz_offset_hours: i32) -> String {
    let epoch_secs = (epoch_ms / 1000) as i64;
    let adjusted_secs = epoch_secs + (tz_offset_hours as i64 * 3600);
    let total_secs = if adjusted_secs >= 0 { adjusted_secs as u64 } else { 0 };

    let sec = (total_secs % 60) as u32;
    let min = ((total_secs / 60) % 60) as u32;
    let hour = ((total_secs / 3600) % 24) as u32;
    let days = (total_secs / 86400) as i64;

    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{min:02}:{sec:02}")
}

/// 获取当前时间戳与本地化时间字符串（根据地区代码自动推算时区，中国大陆默认 UTC+8）
pub fn current_time_and_epoch(locale: &str) -> (String, u64) {
    let now = SystemTime::now();
    let epoch_ms = now
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let tz_offset = match locale {
        "zh_CN" | "zh_HK" | "zh_TW" | "en_SG" | "en_MY" => 8,
        "ja_JP" => 9,
        _ => 8, // 默认采用东八区北京时间
    };

    let time_str = format_epoch_ms(epoch_ms, tz_offset);
    (time_str, epoch_ms)
}

/// 将单条有货记录追加写入到 CSV 文件
///
/// 写入过程保证：
/// 1. 自动创建所需父级目录；
/// 2. 首行若为空或新文件，自动写入 UTF-8 BOM 与 20 列表头；
/// 3. 所有字段经过 RFC 4180 转义；
/// 4. 每次写入立即调用 `flush()`，防止异常退出导致尾部数据丢失。
pub fn record_in_stock(
    target: &Target,
    pickup: Option<&PickupDetails>,
    actions: &[&str],
    interval_seconds: u64,
    consecutive_failures: u32,
    purchase_url: Option<&str>,
) -> Result<PathBuf, String> {
    record_in_stock_to_dir(&get_log_dir(), target, pickup, actions, interval_seconds, consecutive_failures, purchase_url)
}

/// 将记录写入指定目录（便于单元测试重定向）
pub fn record_in_stock_to_dir(
    dir: &Path,
    target: &Target,
    pickup: Option<&PickupDetails>,
    actions: &[&str],
    interval_seconds: u64,
    consecutive_failures: u32,
    purchase_url: Option<&str>,
) -> Result<PathBuf, String> {
    fs::create_dir_all(dir).map_err(|e| format!("创建日志目录失败：{e}"))?;

    let file_path = dir.join(LOG_FILE_NAME);
    let is_new = !file_path.exists()
        || fs::metadata(&file_path)
            .map(|m| m.len() == 0)
            .unwrap_or(false);

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file_path)
        .map_err(|e| format!("打开日志文件失败：{e}"))?;

    // 写入 UTF-8 BOM 与表头
    if is_new {
        file.write_all(b"\xEF\xBB\xBF")
            .map_err(|e| format!("写入 UTF-8 BOM 失败：{e}"))?;
        let header_line = HEADERS.join(",") + "\r\n";
        file.write_all(header_line.as_bytes())
            .map_err(|e| format!("写入表头失败：{e}"))?;
    }

    let (time_str, epoch_ms) = current_time_and_epoch(&target.locale);
    let region_name = region_by_locale(&target.locale)
        .map(|r| r.title)
        .unwrap_or(target.locale.as_str());

    let actions_str = if actions.is_empty() {
        "-".to_string()
    } else {
        actions.join("、")
    };

    let row = [
        time_str.as_str(),
        &epoch_ms.to_string(),
        target.locale.as_str(),
        region_name,
        target.store_number.as_str(),
        target.store_title.as_str(),
        target.product_name.as_str(),
        target.part_number.as_str(),
        target.companion_part.as_deref().unwrap_or("-"),
        target.companion_name.as_deref().unwrap_or("-"),
        target.kit_part.as_deref().unwrap_or("-"),
        "有货 (InStock)",
        pickup.map(|p| p.pickup_display.as_str()).unwrap_or("-"),
        pickup.and_then(|p| p.pickup_quote.as_deref()).unwrap_or("-"),
        pickup.and_then(|p| p.sale_reason.as_deref()).unwrap_or("-"),
        pickup.and_then(|p| p.sale_message.as_deref()).unwrap_or("-"),
        actions_str.as_str(),
        &interval_seconds.to_string(),
        &consecutive_failures.to_string(),
        purchase_url.unwrap_or("-"),
    ];

    let row_line = row
        .iter()
        .map(|field| escape_csv_field(field))
        .collect::<Vec<_>>()
        .join(",")
        + "\r\n";

    file.write_all(row_line.as_bytes())
        .map_err(|e| format!("写入日志记录失败：{e}"))?;
    file.flush()
        .map_err(|e| format!("刷新日志缓冲失败：{e}"))?;

    Ok(file_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 字段转义符合rfc4180() {
        assert_eq!(escape_csv_field("正常文本"), "正常文本");
        assert_eq!(escape_csv_field("带,逗号"), "\"带,逗号\"");
        assert_eq!(escape_csv_field("带\"引号\""), "\"带\"\"引号\"\"\"");
        assert_eq!(escape_csv_field("第一行\n第二行"), "\"第一行\n第二行\"");
        assert_eq!(escape_csv_field("混合,含\"引号\"\r\n换行"), "\"混合,含\"\"引号\"\"\r\n换行\"");
    }

    #[test]
    fn 日期格式化准确() {
        // 1970-01-01 00:00:00 UTC
        assert_eq!(format_epoch_ms(0, 0), "1970-01-01 00:00:00");
        // 1970-01-01 08:00:00 (UTC+8)
        assert_eq!(format_epoch_ms(0, 8), "1970-01-01 08:00:00");
        // 2026-09-15 00:00:00 UTC = 1789430400 秒
        let epoch_ms = 1_789_430_400_000u64;
        assert_eq!(format_epoch_ms(epoch_ms, 0), "2026-09-15 00:00:00");
        assert_eq!(format_epoch_ms(epoch_ms, 8), "2026-09-15 08:00:00");
    }

    #[test]
    fn 追加写入带bom与表头() {
        let temp_dir = std::env::temp_dir().join(format!("apw_test_log_{}", SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        let target = Target {
            locale: "zh_CN".into(),
            store_number: "R683".into(),
            store_title: "上海-环球港".into(),
            part_number: "MG6X4CH/A".into(),
            product_name: "iPhone 17 Pro 256GB 原色钛金属".into(),
            companion_part: None,
            companion_name: None,
            kit_part: None,
        };
        let pickup = PickupDetails {
            pickup_display: "available".into(),
            pickup_quote: Some("今天".into()),
            sale_reason: None,
            sale_message: None,
        };

        let file_path = record_in_stock_to_dir(
            &temp_dir,
            &target,
            Some(&pickup),
            &["提示音", "Bark"],
            30,
            0,
            Some("https://www.apple.com.cn/shop/product/MG6X4CH/A"),
        ).expect("写入成功");

        assert!(file_path.exists());
        let content = fs::read(&file_path).expect("读取内容");
        // 验证 BOM 头
        assert_eq!(&content[0..3], b"\xEF\xBB\xBF");
        let text = String::from_utf8(content[3..].to_vec()).expect("utf-8");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2); // 表头 + 1 条记录
        assert!(lines[0].starts_with("记录时间,时间戳(ms),地区代码,地区名称"));
        assert!(lines[1].contains("MG6X4CH/A"));
        assert!(lines[1].contains("上海-环球港"));
        assert!(lines[1].contains("提示音、Bark"));

        // 再追加一条
        record_in_stock_to_dir(
            &temp_dir,
            &target,
            Some(&pickup),
            &[],
            30,
            0,
            None,
        ).expect("再次写入成功");

        let content2 = fs::read(&file_path).expect("读取内容");
        let text2 = String::from_utf8(content2[3..].to_vec()).expect("utf-8");
        let lines2: Vec<&str> = text2.lines().collect();
        assert_eq!(lines2.len(), 3); // 表头 + 2 条记录

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
