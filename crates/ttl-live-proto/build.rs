use std::error::Error;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::{env, fs};

use prost::Message;
use prost_types::field_descriptor_proto::Type as ProtoFieldType;
use prost_types::{DescriptorProto, FieldDescriptorProto, FileDescriptorSet};

const PROTO_ROOT: &str = "proto/v3";
const INCLUDE_FILE: &str = "v3.rs";
const STAGE_DIR: &str = "proto-stage/v3";
const DESCRIPTOR_FILE: &str = "v3_descriptor.bin";
const REGISTRY_FILE: &str = "schema_registry.rs";

/// Package holding the `Webcast*Message` types that WebSocket methods name.
/// Preferred when a short name exists in more than one package.
const METHOD_PACKAGE: &str = "webcast.model.message";

/// Renames applied to a staged copy of the vendored schemas before codegen.
///
/// The files under `proto/v3/` are kept byte-for-byte identical to upstream, so
/// any deviation we need has to live here, where it is reviewable.
///
/// Upstream flattens nested protobuf types into underscore-separated names.
/// `SubPinCardText_TextType` (used by `chatroom.api.SubPinCardText`) and
/// `SubPinCard_Text_TextType` (used by `chatroom.api.Text`) are distinct proto
/// enums that both normalise to the Rust identifier `SubPinCardTextTextType`,
/// which does not compile. We rename the latter, together with its single
/// reference site. This is purely cosmetic: enum *names* never appear on the
/// wire, and every tag number is left untouched.
const RENAMES: &[(&str, &str, &str)] = &[
    (
        "webcast/model/data/messages.proto",
        "enum SubPinCard_Text_TextType {",
        "enum SubPinCardNestedText_TextType {",
    ),
    (
        "webcast/chatroom/api.proto",
        ".webcast.model.data.SubPinCard_Text_TextType type = 1;",
        ".webcast.model.data.SubPinCardNestedText_TextType type = 1;",
    ),
];

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed={PROTO_ROOT}");

    let protoc = protoc_bin_vendored::protoc_bin_path()?;
    env::set_var("PROTOC", protoc);

    let mut sources = Vec::new();
    collect_proto_files(Path::new(PROTO_ROOT), &mut sources)?;
    sources.sort();
    if sources.is_empty() {
        return Err(format!("no .proto files found under {PROTO_ROOT}").into());
    }
    for file in &sources {
        println!("cargo:rerun-if-changed={}", file.display());
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    let stage = out_dir.join(STAGE_DIR);
    let staged = stage_protos(&sources, &stage)?;
    let descriptor_path = out_dir.join(DESCRIPTOR_FILE);

    let mut config = prost_build::Config::new();
    config.include_file(INCLUDE_FILE);
    config.file_descriptor_set_path(&descriptor_path);
    // TikTok adds fields continuously; unknown fields must never abort a decode,
    // which is prost's default behaviour, so no extra configuration is needed.
    config.compile_protos(&staged, &[&stage])?;

    // The same descriptors also drive the dynamic decoder used for page-owned
    // traffic, where a static struct per message type is the wrong tool: TikTok
    // ships methods newer than any pinned schema, and those must stay readable.
    let descriptor_set = FileDescriptorSet::decode(fs::read(&descriptor_path)?.as_slice())?;
    let (messages, method_count) = collect_schema_info(&descriptor_set)?;
    fs::write(
        out_dir.join(REGISTRY_FILE),
        render_registry(&messages, method_count),
    )?;

    Ok(())
}

#[derive(Clone)]
struct MessageInfo {
    name: String,
    fields: Vec<FieldInfo>,
}

#[derive(Clone)]
struct FieldInfo {
    number: i32,
    name: String,
    kind: FieldKindInfo,
}

#[derive(Clone)]
enum FieldKindInfo {
    Varint,
    Fixed64,
    Fixed32,
    String,
    Bytes,
    Message(String),
}

/// Flattens every message in every file into `package.Name` descriptors.
///
/// Returns the descriptors plus how many of them are addressable as a WebSocket
/// method, which is the number worth watching after a schema update.
fn collect_schema_info(
    descriptor_set: &FileDescriptorSet,
) -> Result<(Vec<MessageInfo>, usize), Box<dyn Error>> {
    let mut messages = Vec::new();
    for file in &descriptor_set.file {
        let package = file.package.as_deref().unwrap_or_default();
        for message in &file.message_type {
            collect_messages(package, message, &mut messages)?;
        }
    }

    let method_count = messages
        .iter()
        .filter(|message| {
            message
                .name
                .rsplit('.')
                .next()
                .is_some_and(|short| short.starts_with("Webcast"))
        })
        .count();

    Ok((messages, method_count))
}

fn collect_messages(
    parent_name: &str,
    message: &DescriptorProto,
    messages: &mut Vec<MessageInfo>,
) -> Result<(), Box<dyn Error>> {
    let short_name = message
        .name
        .as_deref()
        .ok_or("schema message has no name")?;
    let name = if parent_name.is_empty() {
        short_name.to_owned()
    } else {
        format!("{parent_name}.{short_name}")
    };
    let fields = message
        .field
        .iter()
        .filter_map(field_info)
        .collect::<Vec<_>>();
    messages.push(MessageInfo {
        name: name.clone(),
        fields,
    });
    for nested in &message.nested_type {
        collect_messages(&name, nested, messages)?;
    }
    Ok(())
}

fn field_info(field: &FieldDescriptorProto) -> Option<FieldInfo> {
    let number = field.number?;
    if number <= 0 {
        return None;
    }
    let name = field.name.clone()?;
    let kind = match ProtoFieldType::try_from(field.r#type?).ok()? {
        ProtoFieldType::Double | ProtoFieldType::Fixed64 | ProtoFieldType::Sfixed64 => {
            FieldKindInfo::Fixed64
        }
        ProtoFieldType::Float | ProtoFieldType::Fixed32 | ProtoFieldType::Sfixed32 => {
            FieldKindInfo::Fixed32
        }
        ProtoFieldType::String => FieldKindInfo::String,
        ProtoFieldType::Bytes => FieldKindInfo::Bytes,
        ProtoFieldType::Message | ProtoFieldType::Group => FieldKindInfo::Message(
            field
                .type_name
                .as_deref()
                .unwrap_or_default()
                .trim_start_matches('.')
                .to_owned(),
        ),
        ProtoFieldType::Int64
        | ProtoFieldType::Uint64
        | ProtoFieldType::Int32
        | ProtoFieldType::Bool
        | ProtoFieldType::Uint32
        | ProtoFieldType::Enum
        | ProtoFieldType::Sint32
        | ProtoFieldType::Sint64 => FieldKindInfo::Varint,
    };
    Some(FieldInfo { number, name, kind })
}

fn render_registry(messages: &[MessageInfo], method_count: usize) -> String {
    let mut source = String::new();
    source.push_str("// @generated by build.rs from proto/v3. Do not edit manually.\n\n");
    let _ = writeln!(
        source,
        "/// Number of message descriptors in the pinned schema.\n\
         pub const GENERATED_SCHEMA_MESSAGE_COUNT: usize = {};\n",
        messages.len()
    );
    let _ = writeln!(
        source,
        "/// Number of descriptors addressable as a `Webcast*` WebSocket method.\n\
         pub const GENERATED_WEBCAST_METHOD_COUNT: usize = {method_count};\n"
    );

    for (index, message) in messages.iter().enumerate() {
        let _ = writeln!(source, "static FIELDS_{index}: &[FieldSchema] = &[");
        for field in &message.fields {
            let kind = render_field_kind(&field.kind);
            let _ = writeln!(
                source,
                "    FieldSchema {{ number: {}, name: {:?}, kind: {kind} }},",
                field.number, field.name
            );
        }
        source.push_str("];\n\n");
    }

    source.push_str("static SCHEMAS: &[MessageSchema] = &[\n");
    for (index, message) in messages.iter().enumerate() {
        let _ = writeln!(
            source,
            "    MessageSchema {{ name: {:?}, fields: FIELDS_{index} }},",
            message.name
        );
    }
    source.push_str("];\n\n");

    let _ = write!(
        source,
        r#"/// Every descriptor in the pinned schema, in declaration order.
pub fn schemas() -> &'static [MessageSchema] {{
    SCHEMAS
}}

/// Find a descriptor by its fully qualified protobuf name.
pub fn schema_by_name(name: &str) -> Option<&'static MessageSchema> {{
    SCHEMAS.iter().find(|schema| schema.name == name)
}}

/// Find the descriptor for a TikTok WebSocket method.
///
/// In the v3 schema a method name *is* the message name, so `WebcastChatMessage`
/// resolves to `{METHOD_PACKAGE}.WebcastChatMessage`. Short names are not unique
/// across packages, so that package wins and anything else is a fallback; a
/// nested type that merely shares a short name never matches.
pub fn schema_for_method(method: &str) -> Option<&'static MessageSchema> {{
    if !method.starts_with("Webcast") {{
        return None;
    }}
    let qualified = format!("{METHOD_PACKAGE}.{{method}}");
    if let Some(schema) = SCHEMAS.iter().find(|schema| schema.name == qualified) {{
        return Some(schema);
    }}
    SCHEMAS
        .iter()
        .find(|schema| schema.name.rsplit('.').next() == Some(method))
}}
"#
    );
    source
}

fn render_field_kind(kind: &FieldKindInfo) -> String {
    match kind {
        FieldKindInfo::Varint => "FieldKind::Varint".into(),
        FieldKindInfo::Fixed64 => "FieldKind::Fixed64".into(),
        FieldKindInfo::Fixed32 => "FieldKind::Fixed32".into(),
        FieldKindInfo::String => "FieldKind::String".into(),
        FieldKindInfo::Bytes => "FieldKind::Bytes".into(),
        FieldKindInfo::Message(name) => format!("FieldKind::Message({name:?})"),
    }
}

/// Copies the vendored schemas into `OUT_DIR`, applying [`RENAMES`] on the way.
fn stage_protos(sources: &[PathBuf], stage: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    if stage.exists() {
        fs::remove_dir_all(stage)?;
    }

    let root = Path::new(PROTO_ROOT);
    let mut applied = vec![false; RENAMES.len()];
    let mut staged = Vec::with_capacity(sources.len());

    for source in sources {
        let relative = source.strip_prefix(root)?;
        let target = stage.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut contents = fs::read_to_string(source)?;
        for (index, (file, from, to)) in RENAMES.iter().enumerate() {
            if Path::new(file) == relative && contents.contains(from) {
                contents = contents.replace(from, to);
                applied[index] = true;
            }
        }

        fs::write(&target, contents)?;
        staged.push(target);
    }

    // A rename that stops matching means upstream changed underneath us; fail
    // loudly rather than emitting code that silently differs from expectations.
    for (index, (file, from, _)) in RENAMES.iter().enumerate() {
        if !applied[index] {
            return Err(format!("stale rename for {file}: {from:?} not found").into());
        }
    }

    Ok(staged)
}

fn collect_proto_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), Box<dyn Error>> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_proto_files(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "proto") {
            out.push(path);
        }
    }
    Ok(())
}
