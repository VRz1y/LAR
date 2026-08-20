use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AidlFile {
    pub package: Option<String>,
    pub interfaces: Vec<AidlInterface>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AidlInterface {
    pub name: String,
    pub methods: Vec<AidlMethod>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AidlMethod {
    pub name: String,
    pub return_type: String,
    pub parameters: Vec<AidlParameter>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AidlParameter {
    pub direction: ParameterDirection,
    pub ty: String,
    pub name: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParameterDirection {
    In,
    Out,
    InOut,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AidlError {
    pub line: usize,
    pub message: String,
}
impl fmt::Display for AidlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AIDL line {}: {}", self.line, self.message)
    }
}
impl std::error::Error for AidlError {}

pub fn parse(source: &str) -> Result<AidlFile, AidlError> {
    let mut package = None;
    let mut interfaces = Vec::new();
    let mut current = None;
    for (index, raw) in source.lines().enumerate() {
        let line = raw.split("//").next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("import ") {
            let _ = rest;
            continue;
        }
        if let Some(rest) = line.strip_prefix("package ") {
            package = Some(rest.trim_end_matches(';').trim().to_owned());
            continue;
        }
        if line.starts_with("interface ") || line.starts_with("oneway interface ") {
            let name = line
                .split_whitespace()
                .nth(if line.starts_with("oneway") { 2 } else { 1 })
                .unwrap_or("")
                .trim_end_matches('{');
            if name.is_empty() {
                return Err(err(index, "missing interface name"));
            }
            current = Some(AidlInterface {
                name: name.to_owned(),
                methods: Vec::new(),
            });
            continue;
        }
        if line == "}" || line == "};" {
            if let Some(item) = current.take() {
                interfaces.push(item);
            }
            continue;
        }
        if let Some(current) = current.as_mut()
            && line.contains('(')
        {
            let open = line.find('(').unwrap();
            let close = line.rfind(')').ok_or_else(|| err(index, "missing ')'"))?;
            let head: Vec<_> = line[..open].split_whitespace().collect();
            if head.len() < 2 {
                return Err(err(index, "method requires return type and name"));
            }
            let parameters = line[open + 1..close]
                .split(',')
                .filter(|p| !p.trim().is_empty())
                .map(|p| parse_parameter(p, index))
                .collect::<Result<Vec<_>, _>>()?;
            current.methods.push(AidlMethod {
                return_type: head[head.len() - 2].to_owned(),
                name: head[head.len() - 1].to_owned(),
                parameters,
            });
        }
    }
    if let Some(item) = current {
        interfaces.push(item);
    }
    Ok(AidlFile {
        package,
        interfaces,
    })
}

fn parse_parameter(value: &str, line: usize) -> Result<AidlParameter, AidlError> {
    let tokens: Vec<_> = value.split_whitespace().collect();
    let (direction, offset) = match tokens.first().copied() {
        Some("in") => (ParameterDirection::In, 1),
        Some("out") => (ParameterDirection::Out, 1),
        Some("inout") => (ParameterDirection::InOut, 1),
        _ => (ParameterDirection::In, 0),
    };
    if tokens.len() < offset + 2 {
        return Err(err(line, "parameter requires type and name"));
    }
    Ok(AidlParameter {
        direction,
        ty: tokens[offset].to_owned(),
        name: tokens[offset + 1].to_owned(),
    })
}
fn err(line: usize, message: &str) -> AidlError {
    AidlError {
        line: line + 1,
        message: message.to_owned(),
    }
}

pub fn generate_rust(file: &AidlFile) -> String {
    let mut out = String::new();
    if let Some(package) = &file.package {
        out.push_str(&format!("// package: {}\n", package));
    }
    for interface in &file.interfaces {
        out.push_str(&format!("pub trait {} {{\n", interface.name));
        for method in &interface.methods {
            let args = method
                .parameters
                .iter()
                .map(|p| {
                    let ty = rust_type(&p.ty);
                    match p.direction {
                        ParameterDirection::In => format!("{}: {}", p.name, ty),
                        ParameterDirection::Out | ParameterDirection::InOut => {
                            format!("{}: &mut {}", p.name, ty)
                        }
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!(
                "    fn {}(&mut self{}{}) -> {};\n",
                method.name,
                if args.is_empty() { "" } else { ", " },
                args,
                rust_type(&method.return_type)
            ));
        }
        out.push_str("}\n\n");
        out.push_str(&format!(
            "pub struct {}Stub<T> {{ pub implementation: T }}\n",
            interface.name
        ));
    }
    out
}

pub fn generate_hidl_rust(file: &AidlFile) -> String {
    generate_rust(file)
}
fn rust_type(ty: &str) -> String {
    if let Some(element) = ty.strip_suffix("[]") {
        return format!("Vec<{}>", rust_type(element));
    }
    match ty {
        "void" => "()",
        "byte" => "i8",
        "int" => "i32",
        "long" => "i64",
        "float" => "f32",
        "double" => "f64",
        "boolean" => "bool",
        "String" => "String",
        _ => "ParcelValue",
    }
    .to_owned()
}
