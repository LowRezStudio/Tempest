use std::process::ExitCode;

use tempest_upk::flags::{describe, OBJECT_FLAG_NAMES};
use tempest_upk::{Package, PackageIndex, WriteOptions};

fn usage() -> ExitCode {
    eprintln!(
        "uscript — UnrealScript / UPK tool for Paladins\n\n\
         usage:\n  \
           uscript info <pkg>                 package summary\n  \
           uscript names <pkg>                name table\n  \
           uscript imports <pkg>              import table\n  \
           uscript exports <pkg> [filter]     export table (optional substring filter)\n  \
           uscript decompress <pkg> <out>     rewrite uncompressed\n  \
           uscript roundtrip <pkg>            parse → write → parse, report differences\n  \
           uscript check <pkg> [-v]           parse every export object and re-serialise it\n  \
           uscript hexdump <pkg> <idx|path>   dump one export payload\n  \
           uscript bytecode-check <pkg> [-v]  disassemble + reassemble every struct's bytecode\n  \
           uscript disasm <pkg> <idx|path>    print the token tree of one struct\n  \
           uscript decompile <pkg> [-I dir]... [-o outdir] [-j N] [Class]...  decompile classes to .uc\n  \
           uscript compile <srcdir/Pkg> -o out.u [-I dir]... [-P dep]... [--server] [--strip-source] [--cooked] [--compress] [--conform pkg] [--allow-config-defaults] [-v]  compile a package
  uscript dump <pkg> [out.txt] [-I dir]... [--no-pos] [--hash|--hash-full]  normalised dump of every export (for diffing)
  uscript project init <cooked-dir> <project-dir> [-j N]   decompile every script package into <project-dir>/Development/Src/<Pkg>/Classes
  uscript project build <project-dir> [-o outdir] [-j N] [Pkg]...  recompile the project in dependency order, each package conformed to its original
  -j N limits the worker threads (default: all cores, or USCRIPT_JOBS)"
    );
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        return usage();
    }
    let r = match args[0].as_str() {
        "info" if args.len() >= 2 => cmd_info(&args[1]),
        "names" if args.len() >= 2 => cmd_names(&args[1]),
        "imports" if args.len() >= 2 => cmd_imports(&args[1]),
        "exports" if args.len() >= 2 => cmd_exports(&args[1], args.get(2).map(|s| s.as_str())),
        "decompress" if args.len() >= 3 => cmd_decompress(&args[1], &args[2]),
        "roundtrip" if args.len() >= 2 => cmd_roundtrip(&args[1]),
        "check" if args.len() >= 2 => objcmds::check(&args[1], args.iter().any(|a| a == "-v")),
        "hexdump" if args.len() >= 3 => objcmds::hexdump(&args[1], &args[2]),
        "bytecode-check" if args.len() >= 2 => bccmds::bytecode_check(&args[1], args.iter().any(|a| a == "-v")),
        "disasm" if args.len() >= 3 => bccmds::disasm(&args[1], &args[2]),
        "decompile" if args.len() >= 2 => dccmds::decompile(&args[1..]),
        "census" if args.len() >= 2 => bccmds::census(&args[1], args.get(2).and_then(|a| u8::from_str_radix(a.trim_start_matches("0x"), 16).ok())),
        "compile" if args.len() >= 2 => cccmds::compile(&args[1..]),
        "dump" if args.len() >= 2 => dumpcmds::dump(&args[1..]),
        "text" if args.len() >= 3 => textcmds::text(&args[1], &args[2]),
        "project" if args.len() >= 2 => projcmds::project(&args[1..]),
        _ => return usage(),
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_info(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let pkg = Package::load(path)?;
    print!("{}", pkg.summary);
    println!("was compressed:     {}", pkg.was_compressed);
    println!("package name:       {}", pkg.package_name());
    let total: usize = pkg.export_data.iter().map(|d| d.len()).sum();
    println!("export payload:     {} bytes", total);
    println!("header tail:        {} bytes", pkg.header_tail.len());
    for (i, c) in pkg.summary.compressed_chunks.iter().enumerate() {
        println!("chunk {i}:            unc @{} size {} / comp @{} size {}", c.uncompressed_offset, c.uncompressed_size, c.compressed_offset, c.compressed_size);
    }
    println!("additional pkgs:    {:?}", pkg.summary.additional_packages_to_cook);
    for (i, t) in pkg.summary.texture_allocations.iter().enumerate() {
        println!("texture alloc {i}:    {}x{} mips={} format={} flags={:#x} exports={:?}", t.size_x, t.size_y, t.num_mips, t.format, t.tex_create_flags, t.export_indices);
    }
    println!("trailing:           {} bytes", pkg.trailing.len());
    Ok(())
}

fn cmd_names(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let pkg = Package::load(path)?;
    for (i, n) in pkg.names.iter().enumerate() {
        println!("{i:5}  {:#018x}  {}", n.flags, n.name);
    }
    Ok(())
}

fn cmd_imports(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let pkg = Package::load(path)?;
    for (i, imp) in pkg.imports.iter().enumerate() {
        let idx = PackageIndex::import(i);
        println!(
            "{i:5}  {}.{} {}",
            pkg.fname_to_string(imp.class_package),
            pkg.fname_to_string(imp.class_name),
            pkg.object_path(idx)
        );
    }
    Ok(())
}

fn cmd_exports(path: &str, filter: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let pkg = Package::load(path)?;
    for (i, e) in pkg.exports.iter().enumerate() {
        let idx = PackageIndex::export(i);
        let full = pkg.full_name(idx);
        if let Some(f) = filter {
            if !full.to_ascii_lowercase().contains(&f.to_ascii_lowercase()) {
                continue;
            }
        }
        println!(
            "{i:5}  {full}  super={} arch={} size={} @{} flags={} export_flags={:#x}",
            pkg.object_path(e.super_),
            pkg.object_path(e.archetype),
            e.serial_size,
            e.serial_offset,
            describe(e.object_flags, OBJECT_FLAG_NAMES),
            e.export_flags
        );
    }
    Ok(())
}

fn cmd_decompress(path: &str, out: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut pkg = Package::load(path)?;
    pkg.save(out, &WriteOptions::default())?;
    println!("wrote {out}");
    Ok(())
}

fn cmd_roundtrip(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let original = std::fs::read(path)?;
    let mut pkg = Package::parse(&original)?;
    let was_compressed = pkg.was_compressed;
    let bytes = pkg.to_bytes(&WriteOptions::default());
    let pkg2 = Package::parse(&bytes)?;
    let mut problems = 0;
    if pkg.names != pkg2.names {
        println!("names differ");
        problems += 1;
    }
    if pkg.imports != pkg2.imports {
        println!("imports differ");
        problems += 1;
    }
    for (i, (a, b)) in pkg.exports.iter().zip(&pkg2.exports).enumerate() {
        if a != b {
            println!("export {i} differs: {a:?} vs {b:?}");
            problems += 1;
            if problems > 10 {
                break;
            }
        }
    }
    if pkg.export_data != pkg2.export_data {
        println!("export payloads differ");
        problems += 1;
    }
    if pkg.depends != pkg2.depends {
        println!("depends differ");
        problems += 1;
    }
    if !was_compressed {
        if bytes == original {
            println!("byte-identical roundtrip ({} bytes)", bytes.len());
        } else {
            println!("NOT byte-identical: {} vs {} bytes", bytes.len(), original.len());
            let first = bytes.iter().zip(&original).position(|(a, b)| a != b);
            println!("first difference at {:?}", first);
            problems += 1;
        }
    } else {
        println!("compressed source; structural roundtrip {}", if problems == 0 { "ok" } else { "FAILED" });
    }
    if problems == 0 {
        Ok(())
    } else {
        Err(format!("{problems} problem(s)").into())
    }
}

// ---- object layer commands -------------------------------------------------

pub mod objcmds {
    use tempest_unrealscript::objects::{self, ExportInfo, ObjectBody};
    use tempest_upk::{Package, PackageIndex};

    /// Parse every export, verify exact consumption and byte-identical re-serialisation.
    pub fn check(path: &str, verbose: bool) -> Result<(), Box<dyn std::error::Error>> {
        let pkg = Package::load(path)?;
        let layout = objects::probe_layout(&pkg);
        println!("layout: editor_data={}", layout.editor_data);
        let none = tempest_upk::FName::new(pkg.find_name("None").ok_or("no None name")?);
        let mut ok = 0;
        let mut other = 0;
        let mut failed = 0;
        let mut mismatched = 0;
        for (i, e) in pkg.exports.iter().enumerate() {
            let idx = PackageIndex::export(i);
            let cn = pkg.class_name_of(idx);
            let info = ExportInfo { class_name: &cn, object_flags: e.object_flags, is_component: false, native_prefix_len: 0 };
            let mut parsed = objects::parse_export(&pkg.export_data[i], &pkg, info, layout);
            if parsed.is_err() {
                let info2 = ExportInfo { is_component: true, ..info };
                if let Ok(o) = objects::parse_export(&pkg.export_data[i], &pkg, info2, layout) {
                    parsed = Ok(o);
                }
            }
            match parsed {
                Ok(obj) => {
                    if matches!(obj.body, ObjectBody::Other { .. }) {
                        other += 1;
                    } else {
                        ok += 1;
                    }
                    let re = objects::write_export(&obj, layout, none);
                    if re != pkg.export_data[i] {
                        mismatched += 1;
                        if verbose || mismatched <= 5 {
                            println!("re-serialisation mismatch: {} ({} vs {} bytes)", pkg.full_name(idx), re.len(), pkg.export_data[i].len());
                        }
                    }
                }
                Err(err) => {
                    failed += 1;
                    if verbose || failed <= 10 {
                        println!("FAILED {}: {}", pkg.full_name(idx), err);
                    }
                }
            }
        }
        println!("parsed ok: {ok}, generic: {other}, failed: {failed}, re-serialisation mismatches: {mismatched}");
        if failed > 0 || mismatched > 0 {
            return Err("object check failed".into());
        }
        Ok(())
    }

    pub fn hexdump(path: &str, which: &str) -> Result<(), Box<dyn std::error::Error>> {
        let pkg = Package::load(path)?;
        let i = match which.parse::<usize>() {
            Ok(i) => i,
            Err(_) => pkg.find_export(which, None).ok_or("export not found")?,
        };
        let idx = PackageIndex::export(i);
        println!("{} ({} bytes)", pkg.full_name(idx), pkg.export_data[i].len());
        for (off, chunk) in pkg.export_data[i].chunks(16).enumerate() {
            let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
            let asc: String = chunk.iter().map(|&b| if (32..127).contains(&b) { b as char } else { '.' }).collect();
            println!("{:06x}  {:<48}  {}", off * 16, hex.join(" "), asc);
        }
        Ok(())
    }
}

pub mod bccmds {
    use tempest_unrealscript::bytecode::{Args, Assembler, Disassembler, Token};

    fn walk(t: &Token, f: &mut dyn FnMut(&Token)) {
        f(t);
        fn sub_impl(b: &Token, f: &mut dyn FnMut(&Token)) { walk(b, f); }
        fn args_impl(a: &Args, f: &mut dyn FnMut(&Token)) { for x in &a.args { walk(x, f); } if let Some(d) = &a.debug { walk(d, f); } }
        let mut sub = |b: &Box<Token>| sub_impl(b, f);
        match t {
            Token::Return(e) | Token::GotoLabel(e) | Token::BoolVariable(e) | Token::DynArrayLength(e) | Token::InterfaceContext(e) => sub(e),
            Token::Switch { expr, .. } | Token::JumpIfNot { cond: expr, .. } | Token::Assert { expr, .. } | Token::Skip { expr, .. }
            | Token::MetaCast { expr, .. } | Token::DynamicCast { expr, .. } | Token::InterfaceCast { expr, .. } | Token::PrimitiveCast { expr, .. }
            | Token::StructMember { expr, .. } => sub(expr),
            Token::Case { expr: Some(e), .. } => sub(e),
            Token::Let(a, b) | Token::LetBool(a, b) | Token::LetDelegate(a, b) => { sub(a); sub(b); }
            Token::DynArrayElement { index, base } | Token::ArrayElement { index, base } => { sub(index); sub(base); }
            Token::New { outer, name, flags, class, template } => { sub(outer); sub(name); sub(flags); sub(class); sub(template); }
            Token::Context { object, member, .. } => { sub(object); sub(member); }
            Token::VirtualFunction { args: a, .. } | Token::GlobalFunction { args: a, .. } | Token::FinalFunction { args: a, .. }
            | Token::NativeFunction { args: a, .. } | Token::DelegateCmp { args: a, .. } | Token::DelegateFunction { args: a, .. } => { drop(sub); args_impl(a, f); }
            Token::Iterator { expr, .. } => sub(expr),
            Token::StructCmpEq { a, b, .. } | Token::StructCmpNe { a, b, .. } => { sub(a); sub(b); }
            Token::DynArrayInsert { base, index, count, debug } | Token::DynArrayRemove { base, index, count, debug } => { sub(base); sub(index); sub(count); if let Some(d) = debug { sub(d); } }
            Token::DynArrayAdd { base, count, debug } => { sub(base); sub(count); if let Some(d) = debug { sub(d); } }
            Token::DynArrayAddItem { base, item, debug, .. } | Token::DynArrayRemoveItem { base, item, debug, .. } | Token::DynArrayFind { base, item, debug, .. } => { sub(base); sub(item); if let Some(d) = debug { sub(d); } }
            Token::DynArrayInsertItem { base, index, item, debug, .. } => { sub(base); sub(index); sub(item); if let Some(d) = debug { sub(d); } }
            Token::DynArrayFindStruct { base, member, item, debug, .. } => { sub(base); sub(member); sub(item); if let Some(d) = debug { sub(d); } }
            Token::DynArraySort { base, delegate, debug, .. } => { sub(base); sub(delegate); if let Some(d) = debug { sub(d); } }
            Token::DynArrayIterator { array, item, index, .. } => { sub(array); sub(item); sub(index); }
            Token::Conditional { cond, a, b, .. } => { sub(cond); sub(a); sub(b); }
            Token::DefaultParmValue { expr, debug_before, debug_after, .. } => { if let Some(d) = debug_before { sub(d); } sub(expr); if let Some(d) = debug_after { sub(d); } }
            _ => {}
        }
    }

    fn opcode_of(t: &Token) -> u8 {
        let mut a = Assembler::new();
        a.token(t);
        a.w.buf[0]
    }

    /// Count tokens per opcode; with `find`, list the structs containing that opcode.
    pub fn census(path: &str, find: Option<u8>) -> Result<(), Box<dyn std::error::Error>> {
        let pkg = Package::load(path)?;
        let layout = objects::probe_layout(&pkg);
        let none = pkg.find_name("None");
        let mut counts = std::collections::BTreeMap::<u8, usize>::new();
        for (i, e) in pkg.exports.iter().enumerate() {
            let idx = PackageIndex::export(i);
            let cn = pkg.class_name_of(idx);
            if !matches!(cn.as_str(), "Class" | "State" | "Function" | "ScriptStruct") { continue; }
            let info = ExportInfo { class_name: &cn, object_flags: e.object_flags, is_component: false, native_prefix_len: 0 };
            let Ok(obj) = objects::parse_export(&pkg.export_data[i], &pkg, info, layout) else { continue };
            let s = match &obj.body {
                ObjectBody::Class { s, .. } | ObjectBody::State { s, .. } | ObjectBody::Function { s, .. } | ObjectBody::ScriptStruct { s, .. } => s,
                _ => continue,
            };
            if s.script.is_empty() { continue; }
            let Ok(script) = Disassembler::disassemble(&s.script, none) else { continue };
            let mut hit = false;
            for st in &script.statements {
                walk(&st.token, &mut |t| {
                    let op = opcode_of(t);
                    *counts.entry(op).or_default() += 1;
                    if Some(op) == find { hit = true; }
                });
            }
            if hit { println!("{}", pkg.full_name(idx)); }
        }
        if find.is_none() {
            for (op, n) in counts { println!("{op:#04x} {n}"); }
        }
        Ok(())
    }

    use tempest_unrealscript::objects::{self, ExportInfo, ObjectBody};
    use tempest_upk::{Package, PackageIndex};

    pub fn bytecode_check(path: &str, verbose: bool) -> Result<(), Box<dyn std::error::Error>> {
        let pkg = Package::load(path)?;
        let layout = objects::probe_layout(&pkg);
        let none = pkg.find_name("None");
        let (mut ok, mut failed, mut size_mismatch, mut re_mismatch, mut total_bytes) = (0, 0, 0, 0, 0usize);
        for (i, e) in pkg.exports.iter().enumerate() {
            let idx = PackageIndex::export(i);
            let cn = pkg.class_name_of(idx);
            if !matches!(cn.as_str(), "Class" | "State" | "Function" | "ScriptStruct") {
                continue;
            }
            let info = ExportInfo { class_name: &cn, object_flags: e.object_flags, is_component: false, native_prefix_len: 0 };
            let obj = match objects::parse_export(&pkg.export_data[i], &pkg, info, layout) {
                Ok(o) => o,
                Err(_) => continue,
            };
            let s = match &obj.body {
                ObjectBody::Class { s, .. } | ObjectBody::State { s, .. } | ObjectBody::Function { s, .. } | ObjectBody::ScriptStruct { s, .. } => s,
                _ => continue,
            };
            if s.script.is_empty() {
                continue;
            }
            total_bytes += s.script.len();
            match Disassembler::disassemble(&s.script, none) {
                Ok(script) => {
                    if script.mem_size != s.bytecode_size as u32 {
                        size_mismatch += 1;
                        if verbose || size_mismatch <= 5 {
                            println!("mem size mismatch {}: got {} expected {}", pkg.full_name(idx), script.mem_size, s.bytecode_size);
                        }
                    }
                    let (bytes, _) = Assembler::assemble(&script);
                    if bytes != s.script {
                        re_mismatch += 1;
                        if verbose || re_mismatch <= 5 {
                            println!("reassembly mismatch {}", pkg.full_name(idx));
                        }
                    }
                    ok += 1;
                }
                Err(err) => {
                    failed += 1;
                    if verbose || failed <= 10 {
                        println!("FAILED {}: {}", pkg.full_name(idx), err);
                    }
                }
            }
        }
        println!("structs: {ok} ok, {failed} failed, {size_mismatch} size mismatches, {re_mismatch} reassembly mismatches, {total_bytes} bytecode bytes");
        if failed + size_mismatch + re_mismatch > 0 {
            return Err("bytecode check failed".into());
        }
        Ok(())
    }

    pub fn disasm(path: &str, which: &str) -> Result<(), Box<dyn std::error::Error>> {
        let pkg = Package::load(path)?;
        let layout = objects::probe_layout(&pkg);
        let none = pkg.find_name("None");
        let i = match which.parse::<usize>() {
            Ok(i) => i,
            Err(_) => pkg.find_export(which, None).ok_or("export not found")?,
        };
        let idx = PackageIndex::export(i);
        let e = &pkg.exports[i];
        let cn = pkg.class_name_of(idx);
        let info = ExportInfo { class_name: &cn, object_flags: e.object_flags, is_component: false, native_prefix_len: 0 };
        let obj = objects::parse_export(&pkg.export_data[i], &pkg, info, layout)?;
        let s = match &obj.body {
            ObjectBody::Class { s, .. } | ObjectBody::State { s, .. } | ObjectBody::Function { s, .. } | ObjectBody::ScriptStruct { s, .. } => s,
            _ => return Err("not a struct".into()),
        };
        println!("{} ({} disk bytes, {} mem bytes)", pkg.full_name(idx), s.script.len(), s.bytecode_size);
        let script = Disassembler::disassemble(&s.script, none)?;
        let brief = std::env::args().any(|a| a == "--brief");
        for st in &script.statements {
            if brief {
                let t = &st.token;
                let desc = match t {
                    Token::Jump { target } => format!("Jump -> {target:#x}"),
                    Token::JumpIfNot { target, .. } => format!("JumpIfNot -> {target:#x}"),
                    Token::Case { target, expr } => format!("Case -> {target:#x} ({})", if expr.is_some() { "expr" } else { "default" }),
                    Token::Iterator { end, .. } => format!("Iterator end={end:#x}"),
                    Token::DynArrayIterator { end, .. } => format!("DynArrayIterator end={end:#x}"),
                    Token::Switch { .. } => "Switch".into(),
                    Token::Return(e) => format!("Return({})", match &**e { Token::Nothing => "nothing", Token::ReturnNothing(_) => "failsafe", _ => "expr" }),
                    other => format!("{:?}", std::mem::discriminant(other)).replace("Discriminant(", "tok#").replace(")", ""),
                };
                println!("[{:04x}] {desc}", st.mem_offset);
            } else {
                println!("[{:04x}/{:04x}] {:#?}", st.mem_offset, st.disk_offset, st.token);
            }
        }
        Ok(())
    }
}

pub mod dccmds {
    use std::path::PathBuf;
    use tempest_unrealscript::par;
    use tempest_unrealscript::toolchain::decompile_package;

    pub fn decompile(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
        let mut search: Vec<PathBuf> = Vec::new();
        let mut out: Option<PathBuf> = None;
        let mut classes: Vec<String> = Vec::new();
        let mut pkg_path: Option<String> = None;
        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "-I" => { search.push(PathBuf::from(&args[i + 1])); i += 2; }
                "-o" => { out = Some(PathBuf::from(&args[i + 1])); i += 2; }
                "-j" => { par::set_jobs(args[i + 1].parse()?); i += 2; }
                a => { if pkg_path.is_none() { pkg_path = Some(a.to_string()); } else { classes.push(a.to_string()); } i += 1; }
            }
        }
        let pkg_path = pkg_path.ok_or("missing package")?;
        let dp = decompile_package(std::path::Path::new(&pkg_path), &search)?;
        let selected: Vec<_> = dp.classes.iter().filter(|c| classes.is_empty() || classes.iter().any(|n| n.eq_ignore_ascii_case(&c.name))).collect();
        match out {
            Some(dir) => {
                let classes_dir = dir.join(&dp.name).join("Classes");
                std::fs::create_dir_all(&classes_dir)?;
                for c in &selected {
                    std::fs::write(classes_dir.join(format!("{}.uc", c.name)), &c.source)?;
                }
                println!("wrote {} classes to {}", selected.len(), classes_dir.display());
            }
            None => {
                for c in &selected {
                    println!("// ===== {}.uc =====", c.name);
                    println!("{}", c.source);
                }
            }
        }
        Ok(())
    }
}

pub mod cccmds {
    use std::path::PathBuf;
    use tempest_unrealscript::toolchain::{compile_package_dir, CompileOptions};
    use tempest_upk::WriteOptions;

    pub fn compile(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
        let mut search: Vec<PathBuf> = Vec::new();
        let mut out: Option<PathBuf> = None;
        let mut src: Option<PathBuf> = None;
        let mut opts = CompileOptions::default();
        let mut compress = false;
        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "-I" => { search.push(PathBuf::from(&args[i + 1])); i += 2; }
                "-o" => { out = Some(PathBuf::from(&args[i + 1])); i += 2; }
                "-P" => { opts.deps.push(args[i + 1].clone()); i += 2; }
                "--server" => { opts.output.editor_data = false; i += 1; }
                "--strip-source" => { opts.output.keep_source = false; i += 1; }
                "--allow-config-defaults" => { opts.allow_config_defaults = true; i += 1; }
                "--cooked" => { opts.cooked = true; opts.output.cooked = true; opts.output.keep_source = false; i += 1; }
                "--compress" => { compress = true; i += 1; }
                "--net-index-from" | "--conform" => { opts.conform = Some(PathBuf::from(&args[i + 1])); i += 2; }
                "--version" => { opts.output.file_version = args[i + 1].parse()?; i += 2; }
                "-v" => { opts.verbose = true; i += 1; }
                a => { src = Some(PathBuf::from(a)); i += 1; }
            }
        }
        let src = src.ok_or("missing package source directory")?;
        let package_name = src.file_name().map(|s| s.to_string_lossy().to_string()).ok_or("bad source directory")?;
        let result = compile_package_dir(&src, &search, opts)?;
        for w in &result.warnings { eprintln!("warning: {w}"); }
        let out = out.unwrap_or_else(|| PathBuf::from(format!("{package_name}.u")));
        let mut pkg = result.package;
        pkg.save(&out, &WriteOptions { compress, obscure: compress })?;
        println!("wrote {} ({} exports, {} imports, {} names)", out.display(), pkg.exports.len(), pkg.imports.len(), pkg.names.len());
        Ok(())
    }
}

/// Normalised textual dump of a package for structural comparison between
/// compilers: every export rendered with references as object paths.
pub mod dumpcmds {
    use std::fmt::Write as _;
    use tempest_unrealscript::bytecode::Disassembler;
    use tempest_unrealscript::context::{Context, ObjRef};
    use tempest_unrealscript::decompile::Decompiler;
    use tempest_unrealscript::objects::{ObjectBody, ParsedObject, TaggedProperty};
    use tempest_upk::flags::{describe, OBJECT_FLAG_NAMES};
    use tempest_upk::{FName, Package, PackageIndex};

    struct Dumper<'a> {
        ctx: &'a Context,
        slot: usize,
        pkg: &'a Package,
        no_pos: bool,
        hash: bool,
        hash_full: bool,
    }

    impl<'a> Dumper<'a> {
        fn path(&self, i: PackageIndex) -> String {
            if i.is_null() {
                "None".into()
            } else {
                let p = self.pkg.object_path(i);
                if i.import_index().is_some() {
                    format!("{}.{}", self.pkg.outermost_name(i), p.splitn(2, '.').nth(1).unwrap_or(&p))
                } else {
                    p
                }
            }
        }
        fn name(&self, n: FName) -> String {
            self.pkg.fname_to_string(n)
        }
        fn tagged(&self, out: &mut String, props: &[TaggedProperty], owner: Option<ObjRef>, cdo: Option<ObjRef>, obj: Option<ObjRef>, indent: &str) {
            let class = owner.unwrap_or(ObjRef { pkg: self.slot, export: 0 });
            let d = Decompiler::new(self.ctx, class);
            let lines = d.default_lines_of(props, owner, self.slot, cdo, obj);
            for l in lines.lines {
                let _ = writeln!(out, "{indent}{l}");
            }
        }
        fn script(&self, out: &mut String, script: &[u8], mem_size: i32) {
            let none = self.pkg.find_name("None");
            match Disassembler::disassemble(script, none) {
                Ok(s) => {
                    let _ = writeln!(out, "  script: {} disk bytes, {} mem bytes (declared {})", script.len(), s.mem_size, mem_size);
                    for st in &s.statements {
                        let text = format!("{:?}", st.token);
                        let text = self.substitute_refs(&text);
                        let _ = writeln!(out, "    [{:04x}] {}", st.mem_offset, text.replace('\n', " ").split_whitespace().collect::<Vec<_>>().join(" "));
                    }
                }
                Err(e) => {
                    let _ = writeln!(out, "  script: ERROR {e}");
                }
            }
        }
        fn substitute_refs(&self, s: &str) -> String {
            let mut out = String::new();
            let mut rest = s;
            loop {
                let next_e = rest.find("Export(");
                let next_i = rest.find("Import(");
                let next_n = rest.find("FName { index: ");
                let mut cands = vec![];
                if let Some(p) = next_e { cands.push((p, 0)); }
                if let Some(p) = next_i { cands.push((p, 1)); }
                if let Some(p) = next_n { cands.push((p, 2)); }
                let Some(&(pos, kind)) = cands.iter().min_by_key(|(p, _)| *p) else {
                    out.push_str(rest);
                    break;
                };
                out.push_str(&rest[..pos]);
                let tail = &rest[pos..];
                if kind == 2 {
                    let end = tail.find('}').map(|e| e + 1).unwrap_or(tail.len());
                    let body = &tail[..end];
                    let idx: u32 = body.split("index: ").nth(1).and_then(|x| x.split(',').next()).and_then(|x| x.trim().parse().ok()).unwrap_or(0);
                    let num: u32 = body.split("number: ").nth(1).and_then(|x| x.split(' ').next()).and_then(|x| x.trim().parse().ok()).unwrap_or(0);
                    out.push_str(&format!("'{}'", self.name(FName { index: idx, number: num })));
                    rest = &tail[end..];
                } else {
                    let end = tail.find(')').map(|e| e + 1).unwrap_or(tail.len());
                    let n: i32 = tail[7..end - 1].parse().unwrap_or(0);
                    let idx = if kind == 0 { PackageIndex::export(n as usize) } else { PackageIndex::import(n as usize) };
                    out.push_str(&format!("<{}>", self.path(idx)));
                    rest = &tail[end..];
                }
            }
            out
        }
    }

    pub fn dump(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
        let mut ctx = Context::new();
        let mut path: Option<String> = None;
        let mut out_path: Option<String> = None;
        let mut no_pos = false;
        let mut hash = false;
        let mut hash_full = false;
        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "-I" => { ctx.add_search_path(&args[i + 1]); i += 2; }
                "--no-pos" => { no_pos = true; i += 1; }
                "--hash" => { hash = true; i += 1; }
                "--hash-full" => { hash = true; hash_full = true; i += 1; }
                a => { if path.is_none() { path = Some(a.to_string()); } else { out_path = Some(a.to_string()); } i += 1; }
            }
        }
        let path = path.ok_or("missing package")?;
        if let Some(dir) = std::path::Path::new(&path).parent() {
            ctx.add_search_path(dir);
        }
        let slot = ctx.load_file(&path)?;
        ctx.preload_imports(slot);
        let pkg = &ctx.packages[slot].pkg;
        let d = Dumper { ctx: &ctx, slot, pkg, no_pos, hash, hash_full };
        let mut entries: Vec<(String, String)> = Vec::new();
        for (i, e) in pkg.exports.iter().enumerate() {
            let idx = PackageIndex::export(i);
            let cn = pkg.class_name_of(idx);
            // Editor metadata / cooker artifacts are not script objects.
            if cn == "MetaData" || cn == "ShaderCache" {
                continue;
            }
            // Null placeholder exports of conformed packages.
            if e.serial_size == 0 && e.class.is_null() && pkg.object_name(idx).eq_ignore_ascii_case("None") {
                continue;
            }
            let p = pkg.object_path(idx);
            let mut s = String::new();
            let _ = writeln!(s, "== {cn} {p}");
            if d.hash {
                let data = &pkg.export_data[i];
                let mut h: u64 = 0xcbf2_9ce4_8422_2325;
                for &b in data {
                    h ^= b as u64;
                    h = h.wrapping_mul(0x0100_0000_01b3);
                }
                let take = if d.hash_full { data.len() } else { 64 };
                let hex: String = data.iter().take(take).map(|b| format!("{b:02x}")).collect::<Vec<_>>().join("");
                let _ = writeln!(s, "  bytes={} fnv={h:016x} hex={hex}", data.len());
            }
            let _ = writeln!(s, "  super={} outer={} archetype={} flags={}", d.path(e.super_), d.path(e.outer), d.path(e.archetype), describe(e.object_flags & !0x0007_0000_0000_0000, OBJECT_FLAG_NAMES));
            let r = ObjRef { pkg: slot, export: i };
            match ctx.object(r) {
                Some(obj) => d.dump_object(&mut s, obj, &cn, r),
                None => {
                    let data = &pkg.export_data[i];
                    let hex: String = data.iter().take(96).map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ");
                    let _ = writeln!(s, "  (unparsed) size={} hex={hex}", data.len());
                }
            }
            entries.push((format!("{}|{}", p.to_ascii_lowercase(), cn), s));
        }
        entries.sort();
        let text: String = entries.into_iter().map(|(_, s)| s).collect();
        match out_path {
            Some(o) => std::fs::write(o, text)?,
            None => print!("{text}"),
        }
        Ok(())
    }

    impl<'a> Dumper<'a> {
        fn dump_object(&self, s: &mut String, obj: &ParsedObject, cn: &str, r: ObjRef) {
            let h = &obj.header;
            if h.net_index != -1 {
                let _ = writeln!(s, "  net_index={}", h.net_index);
            }
            if let Some(c) = &h.component {
                let _ = writeln!(s, "  component: owner={} name={}", self.path(c.template_owner_class), c.template_name.map(|n| self.name(n)).unwrap_or_default());
            }
            if !h.properties.is_empty() {
                let _ = writeln!(s, "  properties:");
                let class = self.ctx.class_of(r);
                self.tagged(s, &h.properties, class, Some(r), Some(r), "    ");
            }
            if let Some(n) = obj.next {
                let _ = writeln!(s, "  next={}", self.path(n));
            }
            let st = |s: &mut String, d: &tempest_unrealscript::objects::StructData| {
                if self.no_pos {
                    let _ = writeln!(s, "  struct: super={} children={} script_text={} cpp_text={}", self.path(d.super_struct), self.path(d.children), if d.script_text.is_null() { "no" } else { "yes" }, if d.cpp_text.is_null() { "no" } else { "yes" });
                } else {
                    let _ = writeln!(s, "  struct: super={} children={} script_text={} cpp_text={} line={} text_pos={}", self.path(d.super_struct), self.path(d.children), if d.script_text.is_null() { "no" } else { "yes" }, if d.cpp_text.is_null() { "no" } else { "yes" }, d.line, d.text_pos);
                }
                if !d.script.is_empty() {
                    self.script(s, &d.script, d.bytecode_size);
                }
            };
            match &obj.body {
                ObjectBody::Class { s: d, state, class } => {
                    st(s, d);
                    let _ = writeln!(s, "  state: probe_mask={:#x} label_table_offset={} state_flags={:#x} func_map=[{}]", state.probe_mask, state.label_table_offset, state.state_flags, state.func_map.iter().map(|(n, i)| format!("{}={}", self.name(*n), self.path(*i))).collect::<Vec<_>>().join(", "));
                    let _ = writeln!(s, "  class: flags={:#x} within={} config={} components=[{}] interfaces=[{}] dll={} cdo={}", class.class_flags, self.path(class.class_within), self.name(class.class_config_name), class.component_name_to_default_object.iter().map(|(n, i)| format!("{}={}", self.name(*n), self.path(*i))).collect::<Vec<_>>().join(", "), class.interfaces.iter().map(|(a, b)| format!("{}/{}", self.path(*a), self.path(*b))).collect::<Vec<_>>().join(", "), self.name(class.dll_bind_name), self.path(class.class_default_object));
                    if let Some(e) = &class.editor {
                        let names = |v: &Vec<FName>| v.iter().map(|n| self.name(*n)).collect::<Vec<_>>().join(",");
                        let _ = writeln!(s, "  editor: dontsort=[{}] hide=[{}] expand=[{}] collapse=[{}] force_order={} groups=[{}] header='{}'", names(&e.dont_sort_categories), names(&e.hide_categories), names(&e.auto_expand_categories), names(&e.auto_collapse_categories), e.force_script_order, names(&e.class_group_names), e.class_header_filename);
                    }
                }
                ObjectBody::State { s: d, state } => {
                    st(s, d);
                    let _ = writeln!(s, "  state: probe_mask={:#x} label_table_offset={} state_flags={:#x} func_map=[{}]", state.probe_mask, state.label_table_offset, state.state_flags, state.func_map.iter().map(|(n, i)| format!("{}={}", self.name(*n), self.path(*i))).collect::<Vec<_>>().join(", "));
                }
                ObjectBody::Function { s: d, function } => {
                    st(s, d);
                    let _ = writeln!(s, "  function: native={} precedence={} flags={:#x} rep_offset={:?} friendly={}", function.native_index, function.oper_precedence, function.function_flags, function.rep_offset, self.name(function.friendly_name));
                }
                ObjectBody::ScriptStruct { s: d, data } => {
                    st(s, d);
                    let _ = writeln!(s, "  scriptstruct: flags={:#x}", data.struct_flags);
                    if !data.defaults.is_empty() {
                        let _ = writeln!(s, "  defaults:");
                        self.tagged(s, &data.defaults, Some(r), None, None, "    ");
                    }
                }
                ObjectBody::Enum(e) => {
                    let _ = writeln!(s, "  enum: [{}]", e.names.iter().map(|n| self.name(*n)).collect::<Vec<_>>().join(", "));
                }
                ObjectBody::Const(c) => {
                    let _ = writeln!(s, "  const: '{}'", c.value);
                }
                ObjectBody::Property { kind, data } => {
                    let _ = writeln!(s, "  property {kind:?}: dim={} flags={:#x} category={} size_enum={} rep_offset={:?} ref1={} ref2={}", data.array_dim, data.property_flags, data.category.map(|c| self.name(c)).unwrap_or_default(), data.array_size_enum.map(|i| self.path(i)).unwrap_or_default(), data.rep_offset, data.ref1.map(|i| self.path(i)).unwrap_or_default(), data.ref2.map(|i| self.path(i)).unwrap_or_default());
                }
                ObjectBody::TextBuffer(t) => {
                    let _ = writeln!(s, "  textbuffer: pos={} top={} len={} hash={:016x}", t.pos, t.top, t.text.len(), fnv(t.text.as_bytes()));
                }
                ObjectBody::Other { rest } => {
                    let _ = writeln!(s, "  other({cn}): {} bytes", rest.len());
                }
            }
        }
    }

    fn fnv(b: &[u8]) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for &x in b {
            h ^= x as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }
}

pub mod textcmds {
    use tempest_unrealscript::objects::{parse_export, ExportInfo, ObjectBody};
    use tempest_upk::{Package, PackageIndex};
    /// Print the text of a TextBuffer export (by object path).
    pub fn text(path: &str, obj: &str) -> Result<(), Box<dyn std::error::Error>> {
        let pkg = Package::load(path)?;
        let layout = tempest_unrealscript::objects::probe_layout(&pkg);
        let i = pkg.find_export(obj, None).ok_or("export not found")?;
        let e = &pkg.exports[i];
        let cn = pkg.class_name_of(PackageIndex::export(i));
        let info = ExportInfo { class_name: &cn, object_flags: e.object_flags, is_component: false, native_prefix_len: 0 };
        let o = parse_export(&pkg.export_data[i], &pkg, info, layout)?;
        if let ObjectBody::TextBuffer(t) = o.body {
            print!("{}", t.text);
        }
        Ok(())
    }
}

/// A UDK-like project: `Development/Src/<Pkg>/Classes/*.uc` for every script package
/// of a cooked game directory, rebuilt as drop-in replacements.
pub mod projcmds {
    use std::path::{Path, PathBuf};
    use tempest_unrealscript::par;
    use tempest_unrealscript::toolchain::{build_order, script_packages};

    const MANIFEST: &str = "uscript-project.txt";

    pub fn project(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
        match args[0].as_str() {
            "init" if args.len() >= 3 => init(&args[1..]),
            "build" if args.len() >= 2 => build(&args[1..]),
            _ => Err("usage: uscript project init <cooked-dir> <project-dir> [-j N] | build <project-dir> [-o outdir] [-j N] [Pkg]...".into()),
        }
    }

    fn init(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
        let mut positional: Vec<&str> = Vec::new();
        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "-j" => {
                    par::set_jobs(args[i + 1].parse()?);
                    i += 2;
                }
                a => {
                    positional.push(a);
                    i += 1;
                }
            }
        }
        let [cooked, project] = positional[..] else { return Err("usage: uscript project init <cooked-dir> <project-dir> [-j N]".into()) };
        let project = Path::new(project);
        let cooked = Path::new(cooked).canonicalize()?;
        let found = script_packages(&cooked)?;
        for p in found.values() {
            eprintln!("{}: {} classes{}", p.name, p.classes, if p.cooked { "" } else { " (not cooked)" });
        }
        let ordered = build_order(&found)?;
        let src = project.join("Development").join("Src");
        std::fs::create_dir_all(&src)?;
        let mut manifest = format!("cooked {}\n", cooked.display());
        for op in &ordered {
            eprintln!("decompiling {}...", op.name);
            let pkg_path = cooked.join(format!("{}.{}", op.name, op.ext));
            let args: Vec<String> = vec![pkg_path.to_string_lossy().to_string(), "-I".into(), cooked.to_string_lossy().to_string(), "-o".into(), src.to_string_lossy().to_string()];
            super::dccmds::decompile(&args)?;
            manifest.push_str(&format!("package {} {} {} {}\n", op.name, op.ext, if op.cooked { "cooked" } else { "raw" }, op.deps.join(",")));
        }
        std::fs::write(project.join(MANIFEST), manifest)?;
        println!("project written to {} ({} packages): {}", project.display(), ordered.len(), ordered.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(" "));
        Ok(())
    }

    fn build(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
        let project = PathBuf::from(&args[0]);
        let mut out: Option<PathBuf> = None;
        let mut only: Vec<String> = Vec::new();
        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "-o" => {
                    out = Some(PathBuf::from(&args[i + 1]));
                    i += 2;
                }
                "-j" => {
                    par::set_jobs(args[i + 1].parse()?);
                    i += 2;
                }
                a => {
                    only.push(a.to_string());
                    i += 1;
                }
            }
        }
        let out = out.unwrap_or_else(|| project.join("Cooked"));
        std::fs::create_dir_all(&out)?;
        let manifest = std::fs::read_to_string(project.join(MANIFEST))?;
        let mut cooked: Option<PathBuf> = None;
        let mut packages: Vec<(String, String, Vec<String>, bool)> = Vec::new();
        for line in manifest.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            match parts.as_slice() {
                ["cooked", dir] => cooked = Some(PathBuf::from(dir)),
                ["package", name, ext, kind, deps] => packages.push((name.to_string(), ext.to_string(), deps.split(',').filter(|d| !d.is_empty()).map(|d| d.to_string()).collect(), *kind == "cooked")),
                ["package", name, ext, kind] => packages.push((name.to_string(), ext.to_string(), Vec::new(), *kind == "cooked")),
                _ => {}
            }
        }
        let cooked = cooked.ok_or("manifest has no cooked directory")?;
        let src = project.join("Development").join("Src");
        packages.retain(|(name, ..)| only.is_empty() || only.iter().any(|o| o.eq_ignore_ascii_case(name)));
        // A package waits for the dependencies built in this run (its `-I` search
        // path lists the output directory first); the rest come from the game dir.
        let deps: Vec<Vec<usize>> = packages
            .iter()
            .map(|(_, _, d, _)| d.iter().filter_map(|d| packages.iter().position(|(n, ..)| n.eq_ignore_ascii_case(d))).collect())
            .collect();
        let errors = par::schedule(&deps, |i| {
            let (name, ext, deps, is_cooked) = &packages[i];
            eprintln!("compiling {name}...");
            let started = std::time::Instant::now();
            let mut args: Vec<String> = vec![
                src.join(name).to_string_lossy().to_string(),
                "-o".into(),
                out.join(format!("{name}.{ext}")).to_string_lossy().to_string(),
                // Rebuilt dependencies first, then the game's own packages.
                "-I".into(),
                out.to_string_lossy().to_string(),
                "-I".into(),
                cooked.to_string_lossy().to_string(),
            ];
            for d in deps {
                args.push("-P".into());
                args.push(d.clone());
            }
            if deps.is_empty() {
                // No dependencies at all (Core): an empty list would mean "the defaults".
                args.push("-P".into());
                args.push("None".into());
            }
            // A cooked original gets cooked output; a plain (mod) package keeps its source text.
            let flags: &[&str] = if *is_cooked { &["--cooked", "--strip-source", "--allow-config-defaults", "--compress", "--conform"] } else { &["--allow-config-defaults", "--conform"] };
            for f in flags {
                args.push((*f).into());
            }
            args.push(cooked.join(format!("{name}.{ext}")).to_string_lossy().to_string());
            let r = super::cccmds::compile(&args).map_err(|e| format!("{name}: {e}"));
            eprintln!("compiled {name} in {:.1}s", started.elapsed().as_secs_f64());
            r
        });
        match errors.into_iter().next() {
            Some((_, e)) => Err(e.into()),
            None => Ok(()),
        }
    }
}
