use regex::Regex;

/// 结构化抽取结果
#[derive(Debug, Clone, Default)]
pub struct ExtractedData {
    pub id_card: Option<IdCardFields>,
    pub biz_license: Option<BizLicenseFields>,
    pub contract: Option<ContractFields>,
}

#[derive(Debug, Clone, Default)]
pub struct IdCardFields {
    pub name: Option<String>,
    pub id_number: Option<String>,
    pub address: Option<String>,
    pub gender: Option<String>,
    pub birth_date: Option<String>,
    pub valid_through: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct BizLicenseFields {
    pub company_name: Option<String>,
    pub credit_code: Option<String>,
    pub legal_person: Option<String>,
    pub address: Option<String>,
    pub established_date: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ContractFields {
    pub party_a: Option<String>,
    pub party_a_id: Option<String>,
    pub party_b: Option<String>,
    pub party_b_id: Option<String>,
    pub address: Option<String>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub rent: Option<String>,
    pub sign_date: Option<String>,
}

/// 入口：对 OCR 文本做轻量抽取
pub fn extract_all(text: &str) -> ExtractedData {
    ExtractedData {
        id_card: extract_id_card(text),
        biz_license: extract_biz_license(text),
        contract: extract_contract(text),
    }
}

fn extract_id_card(text: &str) -> Option<IdCardFields> {
    let id_re =
        Regex::new(r"(?i)(\d{6})(\d{8})(\d{3})([\dxX])").expect("id regex compile should succeed");
    let name_re = Regex::new(r"(姓名|名称)[:：]?\s*([\p{Han}A-Za-z·]+)")
        .expect("name regex compile should succeed");
    let addr_re =
        Regex::new(r"(住址|地址)[:：]?\s*([\p{Han}A-Za-z0-9#\-\s]+)").expect("addr regex");
    let gender_re =
        Regex::new(r"(性别)[:：]?\s*([男女MF])").expect("gender regex compile should succeed");
    let birth_re =
        Regex::new(r"(出生|生日)[:：]?\s*([0-9]{4}[年./-][0-9]{1,2}[月./-][0-9]{1,2}日?)")
            .expect("birth regex");
    let valid_re =
        Regex::new(r"(有效期限|有效期|至)[:：]?\s*([0-9]{4}.*?[\dXx]{4,})").expect("valid regex");

    let id_number = id_re
        .captures(text)
        .and_then(|c| c.get(0).map(|m| m.as_str().to_string()));

    if id_number.is_none() && !text.contains("身份证") {
        return None;
    }

    let mut fields = IdCardFields::default();
    fields.id_number = id_number;
    fields.name = name_re
        .captures(text)
        .and_then(|c| c.get(2).map(|m| m.as_str().trim().to_string()));
    fields.address = addr_re
        .captures(text)
        .and_then(|c| c.get(2).map(|m| m.as_str().trim().to_string()));
    fields.gender = gender_re
        .captures(text)
        .and_then(|c| c.get(2).map(|m| m.as_str().trim().to_string()));
    fields.birth_date = birth_re
        .captures(text)
        .and_then(|c| c.get(2).map(|m| m.as_str().trim().to_string()));
    fields.valid_through = valid_re
        .captures(text)
        .and_then(|c| c.get(2).map(|m| m.as_str().trim().to_string()));

    Some(fields)
}

fn extract_biz_license(text: &str) -> Option<BizLicenseFields> {
    let credit_re = Regex::new(r"([A-Z0-9]{18})").expect("credit regex compile should succeed");
    let name_re =
        Regex::new(r"(名称|公司名称)[:：]?\s*([\p{Han}A-Za-z0-9（）()·]+)").expect("name regex");
    let legal_re =
        Regex::new(r"(法定代表人|负责人)[:：]?\s*([\p{Han}A-Za-z·]+)").expect("legal regex");
    let addr_re =
        Regex::new(r"(住所|地址)[:：]?\s*([\p{Han}A-Za-z0-9#\-\s]+)").expect("addr regex");
    let date_re =
        Regex::new(r"([0-9]{4}[年./-][0-9]{1,2}[月./-][0-9]{1,2}日?)").expect("date regex");

    if !text.contains("营业执照") && !text.contains("统一社会信用代码") {
        return None;
    }

    let mut fields = BizLicenseFields::default();
    fields.credit_code = credit_re
        .captures(text)
        .and_then(|c| c.get(1).map(|m| m.as_str().trim().to_string()));
    fields.company_name = name_re
        .captures(text)
        .and_then(|c| c.get(2).map(|m| m.as_str().trim().to_string()));
    fields.legal_person = legal_re
        .captures(text)
        .and_then(|c| c.get(2).map(|m| m.as_str().trim().to_string()));
    fields.address = addr_re
        .captures(text)
        .and_then(|c| c.get(2).map(|m| m.as_str().trim().to_string()));
    fields.established_date = date_re
        .captures(text)
        .and_then(|c| c.get(1).map(|m| m.as_str().trim().to_string()));

    Some(fields)
}

fn extract_contract(text: &str) -> Option<ContractFields> {
    // 甲乙方/出租承租
    let party_re =
        Regex::new(r"(甲方|出租人|委托人)[:：]?\s*([\p{Han}A-Za-z·]+)").expect("party regex");
    let party_b_re =
        Regex::new(r"(乙方|承租人|受托人)[:：]?\s*([\p{Han}A-Za-z·]+)").expect("partyB regex");
    let id_re =
        Regex::new(r"(身份证号|公民身份号码|证件号码)[:：]?\s*([0-9Xx]{6,})").expect("id regex");
    let addr_re = Regex::new(r"(房屋地址|租赁地址|地址)[:：]?\s*([\p{Han}A-Za-z0-9#\-\s]+)")
        .expect("addr regex");
    let date_re =
        Regex::new(r"([0-9]{4}[年./-][0-9]{1,2}[月./-][0-9]{1,2}日?)").expect("date regex");
    let rent_re = Regex::new(r"(租金|租赁费用|金额)[:：]?\s*([0-9]+[.,]?[0-9]*\s*[元￥]?\\b)")
        .expect("rent regex");

    if !(text.contains("合同") || text.contains("协议") || text.contains("承租")) {
        return None;
    }

    let mut fields = ContractFields::default();
    fields.party_a = party_re
        .captures(text)
        .and_then(|c| c.get(2).map(|m| m.as_str().trim().to_string()));
    fields.party_b = party_b_re
        .captures(text)
        .and_then(|c| c.get(2).map(|m| m.as_str().trim().to_string()));
    // 尝试提取首个身份证号作为甲方，再提取第二个作为乙方（粗略）
    let mut ids = id_re
        .captures_iter(text)
        .filter_map(|c| c.get(2))
        .map(|m| m.as_str().trim().to_string());
    fields.party_a_id = ids.next();
    fields.party_b_id = ids.next();
    fields.address = addr_re
        .captures(text)
        .and_then(|c| c.get(2).map(|m| m.as_str().trim().to_string()));

    let mut dates = date_re
        .captures_iter(text)
        .filter_map(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string());
    fields.start_date = dates.next();
    fields.end_date = dates.next();
    fields.sign_date = dates.next();

    fields.rent = rent_re
        .captures(text)
        .and_then(|c| c.get(2).map(|m| m.as_str().trim().to_string()));

    Some(fields)
}
