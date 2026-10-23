use std::path::{Path, PathBuf};
use std::process::ExitCode;

use tempest_assets::dump::Dumper;
use tempest_assets::object::{package_layout, parse_export};
use tempest_assets::reflect::ClassDb;
use tempest_assets::verify::{merge_classes, verify_dir, verify_package};
use tempest_unrealscript::par;
use tempest_upk::{Package, PackageFile, PackageIndex, WriteOptions};

type Res = Result<(), Box<dyn std::error::Error>>;

fn usage() -> ExitCode {
    eprintln!(
        "upk — package / asset tool for Paladins\n\n\
         usage:\n  \
           upk info <pkg>                              summary, chunks, texture allocations\n  \
           upk ls <pkg> [filter]                       export table (substring filter on class or path)\n  \
           upk dump <pkg> [<idx|path>] [-I dir]        typed properties of one export (or all)\n  \
           upk extract <pkg> <idx|path> <out>          write one export's raw payload\n  \
           upk decompress <pkg> <out>                  rewrite uncompressed\n  \
           upk compress <pkg> <out> [--obscure]        rewrite LZO-compressed like the cooker\n  \
           upk verify <dir|pkg>... [-I dir] [-j N] [--compressed] [--classes] [-v]\n                                              \
                                               round-trip every export and report coverage\n  \
           upk texture info <pkg> <idx|path> [-I dir]     format, size, where every mip lives\n  \
           upk texture export <pkg> <idx|path> <out.dds|out.png> [--mip N] [-I dir]\n  \
           upk texture export-all <pkg> <outdir> [--png] [-I dir]   every Texture2D of the package\n  \
           upk texture import <pkg> <idx|path> <in.png|in.dds> -o <out.upk> [--mips|--no-mips] [--format PF_DXT5] [-I dir]\n  \
           upk swf export <pkg> <idx|path> <out.gfx>          the compiled Scaleform movie (RawData)\n  \
           upk swf import <pkg> <idx|path> <in.gfx|swf> -o <out.upk>\n  \
           upk sound export <pkg> <idx|path> <out.ogg>        PC audio of a SoundNodeWave\n  \
           upk sound import <pkg> <idx|path> <in.ogg> -o <out.upk>\n  \
           upk mesh info <pkg> <idx|path>                    LODs, sections, bones of a static/skeletal mesh\n  \
           upk mesh export <pkg> <idx|path> <out.glb|out.obj> [--lod N] [--no-textures] [--no-skin]\n  \
           upk mesh import <pkg> <idx|path> <in.glb|in.gltf|in.obj> -o <out.upk> [--lod N]   replace the geometry\n  \
           upk inject <pkg> <idx|path> <in.bin> -o <out.upk>   replace an export's raw payload (inverse of extract)\n  \
           upk prop get <pkg> <idx|path> [Prop.Path[2].Member]\n  \
           upk prop set <pkg> <idx|path> <Prop.Path> <value> [<Prop.Path> <value>]... -o <out.upk>\n  \
           upk obj rename <pkg> <idx|path> <NewName> -o <out.upk>\n  \
           upk obj delete <pkg> <idx|path>... -o <out.upk>        (sub-objects too; references cleared)\n  \
           upk obj copy <src.upk> <idx|path> <dst.upk> -o <out.upk> [--outer <path>|--top] [--shallow]\n  \
           upk deps <pkg> <idx|path>                           what it references, what references it\n  \
           upk mod-dir <dir> --id <id> --name <name> [--version 1.0.0] [--author <name>] <file.upk>...\n                                              \
                                               mod folder for `tempest mod pack` (files/, manifest.toml)\n  \
           upk serve                                  JSON-lines RPC over stdin/stdout (the Studio backend)\n\n\
         -I dir: directory holding the script packages (default: the package's own directory)\n\
         -j N: worker threads (default: all cores, or USCRIPT_JOBS)"
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
        "ls" if args.len() >= 2 => cmd_ls(&args[1], args.get(2).map(|s| s.as_str())),
        "dump" if args.len() >= 2 => cmd_dump(&args[1..]),
        "extract" if args.len() >= 4 => cmd_extract(&args[1], &args[2], &args[3]),
        "inject" if args.len() >= 4 => cmd_inject(&args[1..]),
        "decompress" if args.len() >= 3 => cmd_rewrite(&args[1], &args[2], false, false),
        "compress" if args.len() >= 3 => cmd_rewrite(&args[1], &args[2], true, args.iter().any(|a| a == "--obscure")),
        "verify" if args.len() >= 2 => cmd_verify(&args[1..]),
        "texture" if args.len() >= 3 => cmd_texture(&args[1..]),
        "mesh" if args.len() >= 3 => cmd_mesh(&args[1..]),
        "prop" if args.len() >= 4 => cmd_prop(&args[1..]),
        "obj" if args.len() >= 4 => cmd_obj(&args[1..]),
        "deps" if args.len() >= 3 => cmd_deps(&args[1], &args[2], &args[1..]),
        "mod-dir" if args.len() >= 2 => cmd_mod_dir(&args[1..]),
        "swf" if args.len() >= 4 => cmd_media(&args[1..], true),
        "sound" if args.len() >= 4 => cmd_media(&args[1..], false),
        "imagediff" if args.len() >= 2 => cmd_imagediff(&args[1]),
        "native-debug" if args.len() >= 3 => cmd_native_debug(&args[1], &args[2], &args[1..]),
        "serve" => {
            tempest_assets::serve::main();
            return ExitCode::SUCCESS;
        }
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

/// `-I dir` or the package's directory.
fn script_dir(args: &[String], pkg_path: &str) -> PathBuf {
    if let Some(i) = args.iter().position(|a| a == "-I") {
        if let Some(d) = args.get(i + 1) {
            return PathBuf::from(d);
        }
    }
    Path::new(pkg_path).parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from("."))
}

fn load_db(dir: &Path) -> Result<ClassDb, Box<dyn std::error::Error>> {
    let t = std::time::Instant::now();
    let db = ClassDb::load_dir(dir)?;
    eprintln!("class db: {} packages from {} ({:.1}s)", db.package_names().len(), dir.display(), t.elapsed().as_secs_f32());
    Ok(db)
}

fn cmd_info(path: &str) -> Res {
    let pf = PackageFile::open(path)?;
    let pkg = pf.tables();
    print!("{}", pkg.summary);
    println!("was compressed:     {}", pkg.was_compressed);
    println!("package name:       {}", pkg.package_name());
    println!("image size:         {} bytes", pf.image_len());
    println!("header tail:        {} bytes", pkg.header_tail.len());
    for (i, c) in pkg.summary.compressed_chunks.iter().enumerate() {
        println!("chunk {i}:            unc @{} size {} / comp @{} size {}", c.uncompressed_offset, c.uncompressed_size, c.compressed_offset, c.compressed_size);
    }
    println!("additional pkgs:    {:?}", pkg.summary.additional_packages_to_cook);
    for (i, t) in pkg.summary.texture_allocations.iter().enumerate() {
        println!("texture alloc {i}:    {}x{} mips={} format={} flags={:#x} exports={:?}", t.size_x, t.size_y, t.num_mips, t.format, t.tex_create_flags, t.export_indices);
    }
    Ok(())
}

fn cmd_ls(path: &str, filter: Option<&str>) -> Res {
    let pf = PackageFile::open(path)?;
    let pkg = pf.tables();
    let d = Dumper::new(pkg);
    for i in 0..pkg.exports.len() {
        let line = d.export_line(i);
        if let Some(f) = filter {
            if !line.to_ascii_lowercase().contains(&f.to_ascii_lowercase()) {
                continue;
            }
        }
        println!("{line}");
    }
    Ok(())
}

/// Resolve `<idx|path>` to an export index.
fn find_export(pkg: &Package, what: &str) -> Result<usize, Box<dyn std::error::Error>> {
    if let Ok(i) = what.parse::<usize>() {
        if i < pkg.exports.len() {
            return Ok(i);
        }
        return Err(format!("export {i} out of range (have {})", pkg.exports.len()).into());
    }
    pkg.find_export(what, None).ok_or_else(|| format!("no export named {what}").into())
}

fn cmd_dump(args: &[String]) -> Res {
    let path = &args[0];
    let what = args.get(1).filter(|a| !a.starts_with('-')).map(|s| s.as_str());
    let db = load_db(&script_dir(args, path))?;
    let pf = PackageFile::open(path)?;
    let pkg = pf.load()?;
    let d = Dumper::new(&pkg);
    let layout = package_layout(&pkg);
    let range: Vec<usize> = match what {
        Some(w) => vec![find_export(&pkg, w)?],
        None => (0..pkg.exports.len()).collect(),
    };
    let mut out = String::new();
    for i in range {
        out.clear();
        out.push_str(&d.export_line(i));
        out.push('\n');
        let p = parse_export(&db, &pkg, layout, i, &pkg.export_data[i]);
        d.parsed(&p, &mut out);
        print!("{out}");
    }
    Ok(())
}

fn cmd_extract(path: &str, what: &str, out: &str) -> Res {
    let pf = PackageFile::open(path)?;
    let i = find_export(pf.tables(), what)?;
    let data = pf.export_data(i)?;
    std::fs::write(out, &data)?;
    eprintln!("wrote {} bytes of {} to {out}", data.len(), pf.tables().full_name(PackageIndex::export(i)));
    Ok(())
}

fn cmd_inject(args: &[String]) -> Res {
    use tempest_assets::edit::Editor;
    let path = &args[0];
    let db = load_db(&script_dir(args, path))?;
    let mut ed = Editor::open(&db, path)?;
    let i = ed.find(&args[1])?;
    let out = out_arg(args)?;
    let bytes = std::fs::read(&args[2])?;
    eprintln!("replacing {} ({} bytes) with {} bytes", ed.pkg.full_name(PackageIndex::export(i)), ed.pkg.export_data[i].len(), bytes.len());
    ed.set_raw(i, bytes)?;
    ed.save(&out)?;
    eprintln!("wrote {out}");
    Ok(())
}

fn cmd_rewrite(path: &str, out: &str, compress: bool, obscure: bool) -> Res {
    let pf = PackageFile::open(path)?;
    let mut pkg = pf.load()?;
    let obscure = obscure || (compress && pkg.summary.compression_flags & tempest_upk::flags::compression::OBSCURED != 0);
    pkg.save(out, &WriteOptions { compress, obscure })?;
    Ok(())
}

fn cmd_verify(args: &[String]) -> Res {
    let mut targets: Vec<PathBuf> = Vec::new();
    let mut script: Option<PathBuf> = None;
    let mut check_compressed = false;
    let mut classes = false;
    let mut verbose = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-I" => {
                script = args.get(i + 1).map(PathBuf::from);
                i += 1;
            }
            "-j" => {
                if let Some(n) = args.get(i + 1).and_then(|v| v.parse().ok()) {
                    par::set_jobs(n);
                }
                i += 1;
            }
            "--compressed" => check_compressed = true,
            "--classes" => classes = true,
            "-v" => verbose = true,
            a => targets.push(PathBuf::from(a)),
        }
        i += 1;
    }
    if targets.is_empty() {
        return Err("nothing to verify".into());
    }
    let script_dir = script.unwrap_or_else(|| {
        let t = &targets[0];
        if t.is_dir() { t.clone() } else { t.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from(".")) }
    });
    let db = load_db(&script_dir)?;
    let t0 = std::time::Instant::now();
    let mut reports = Vec::new();
    for t in &targets {
        if t.is_dir() {
            reports.extend(verify_dir(&db, t, check_compressed));
        } else {
            reports.push(verify_package(&db, t, check_compressed));
        }
    }
    let mut bad = 0;
    let mut recompressed = (0usize, 0usize);
    let (mut exports, mut full, mut raw, mut partial, mut tail, mut mism) = (0, 0, 0, 0, 0, 0);
    for r in &reports {
        exports += r.exports;
        full += r.full;
        raw += r.raw;
        partial += r.partial_values;
        tail += r.with_tail;
        mism += r.mismatches;
        // Re-compression is informational: the cooker uses LZO Professional
        // (`lzopro_lzo1x_1_14_compress`), whose output our LZO1X encoder
        // cannot reproduce bit-for-bit; the game accepts either stream.
        let ok = r.error.is_none() && r.mismatches == 0 && r.image_identical;
        if !ok {
            bad += 1;
        }
        if let Some(c) = r.compressed_identical {
            recompressed.0 += c as usize;
            recompressed.1 += 1;
        }
        if verbose || !ok {
            let mut flags = Vec::new();
            if let Some(e) = &r.error {
                flags.push(format!("ERROR {e}"));
            }
            if r.mismatches > 0 {
                flags.push(format!("{} export mismatches", r.mismatches));
            }
            if !r.image_identical && r.error.is_none() {
                flags.push("image differs".into());
            }
            if r.compressed_identical == Some(false) {
                flags.push("(recompressed bytes differ: LZO Pro)".into());
            }
            println!(
                "{:<48} exports={:<6} full={:<6} raw={:<4} partial={:<5} tail={:<6} {}",
                r.name,
                r.exports,
                r.full,
                r.raw,
                r.partial_values,
                r.with_tail,
                flags.join(", ")
            );
            for s in &r.mismatch_samples {
                println!("    mismatch: {s}");
            }
            for s in &r.raw_samples {
                println!("    unparsed: {s}");
            }
        }
    }
    println!();
    println!("packages: {} ({} with problems)   time: {:.1}s", reports.len(), bad, t0.elapsed().as_secs_f32());
    println!("exports:  {exports}");
    println!("  fully understood:        {full} ({:.2}%)", pct(full, exports));
    println!("  with native tail:        {tail} ({:.2}%)", pct(tail, exports));
    println!("  with raw property values:{partial} ({:.2}%)", pct(partial, exports));
    println!("  header unparsed:         {raw} ({:.2}%)", pct(raw, exports));
    println!("  round-trip mismatches:   {mism}");
    if recompressed.1 > 0 {
        println!("  recompressed identical:  {} of {} (informational; the cooker's LZO Pro stream differs)", recompressed.0, recompressed.1);
    }
    if classes {
        println!();
        println!("{:<48} {:>8} {:>8} {:>6} {:>8} {:>8} {:>12} {:>6}", "class", "exports", "full", "raw", "partial", "tail", "tail bytes", "mism");
        let merged = merge_classes(&reports);
        let mut rows: Vec<_> = merged.into_iter().collect();
        rows.sort_by(|a, b| (b.1.exports - b.1.full).cmp(&(a.1.exports - a.1.full)).then(a.0.cmp(&b.0)));
        for (name, s) in rows {
            println!("{:<48} {:>8} {:>8} {:>6} {:>8} {:>8} {:>12} {:>6}", name, s.exports, s.full, s.raw, s.partial_values, s.with_tail, s.tail_bytes, s.mismatches);
        }
    }
    if bad > 0 {
        return Err(format!("{bad} package(s) did not round-trip").into());
    }
    Ok(())
}

fn pct(n: usize, d: usize) -> f64 {
    if d == 0 { 0.0 } else { n as f64 * 100.0 / d as f64 }
}

/// Debug: where does a rebuilt image first differ from the original?
fn cmd_imagediff(path: &str) -> Res {
    let pf = PackageFile::open(path)?;
    let mut pkg = pf.load()?;
    let total = pf.image_len();
    let orig = pf.read_range(0, total)?.into_owned();
    let s0 = pkg.summary.clone();
    let out = pkg.to_bytes(&WriteOptions { compress: false, obscure: false });
    println!("original image {} bytes, rebuilt {} bytes", orig.len(), out.len());
    let first = orig.iter().zip(out.iter()).position(|(a, b)| a != b);
    let region = |off: usize| -> String {
        let s = &s0;
        if off < s.name_offset as usize { return "summary".into(); }
        if off < s.import_offset as usize { return "names".into(); }
        if off < s.export_offset as usize { return "imports".into(); }
        if off < s.depends_offset as usize { return "exports table".into(); }
        if off < s.total_header_size as usize { return "depends/guids/header tail".into(); }
        for (i, e) in pkg.exports.iter().enumerate() {
            let a = e.serial_offset as usize;
            if off >= a && off < a + e.serial_size as usize { return format!("export {i} ({}) +{}", pkg.full_name(PackageIndex::export(i)), off - a); }
        }
        "gap between payloads".into()
    };
    let after = s0.name_offset.max(0) as usize;
    let first_tables = orig[after..].iter().zip(out[after..].iter()).position(|(a, b)| a != b).map(|p| p + after);
    if let Some(off) = first_tables {
        println!("first difference after the summary at {off} [orig region: {}]: orig {:02x?} ours {:02x?}", region(off), &orig[off..(off + 16).min(orig.len())], &out[off..(off + 16).min(out.len())]);
    } else {
        println!("tables and payloads identical");
    }
    match first {
        Some(off) => println!("first difference at {off} ({}) [orig region: {}]", off, region(off)),
        None if orig.len() == out.len() => println!("identical"),
        None => println!("common prefix identical; length differs"),
    }
    // Layout facts.
    let mut offs: Vec<(i32, i32, usize)> = pkg.exports.iter().enumerate().map(|(i, e)| (e.serial_offset, e.serial_size, i)).collect();
    let monotonic = offs.windows(2).all(|w| w[0].0 <= w[1].0);
    println!("export payloads in table order: {monotonic}");
    offs.sort();
    let mut gaps = 0;
    let mut cur = s0.total_header_size as usize;
    for &(o, sz, _) in &offs {
        if o as usize > cur { gaps += 1; }
        cur = cur.max(o as usize + sz as usize);
    }
    println!("gaps between payloads: {gaps}; trailing: {} bytes; header tail: {} bytes", pkg.trailing.len(), pkg.header_tail.len());
    if !s0.compressed_chunks.is_empty() {
        let obscure = s0.compression_flags & tempest_upk::flags::compression::OBSCURED != 0;
        pkg.summary = s0.clone();
        let comp = pkg.to_bytes(&WriteOptions { compress: true, obscure });
        let origf = std::fs::read(path)?;
        println!("compressed: original {} bytes, rebuilt {} bytes, chunks {} vs {}", origf.len(), comp.len(), s0.compressed_chunks.len(), pkg.summary.compressed_chunks.len());
        for (i, (a, b)) in s0.compressed_chunks.iter().zip(pkg.summary.compressed_chunks.iter()).enumerate().take(6) {
            println!("  chunk {i}: orig unc {}+{} comp {}+{} | ours unc {}+{} comp {}+{}", a.uncompressed_offset, a.uncompressed_size, a.compressed_offset, a.compressed_size, b.uncompressed_offset, b.uncompressed_size, b.compressed_offset, b.compressed_size);
        }
        if let Some(off) = origf.iter().zip(comp.iter()).position(|(a, b)| a != b) {
            println!("  first compressed difference at {off}");
        }
    }
    Ok(())
}

fn cmd_texture(args: &[String]) -> Res {
    use tempest_assets::object::{Native, Parsed};
    use tempest_assets::texcache::{MipLocation, TextureSource};
    use tempest_assets::texture_ops;
    let sub = args[0].as_str();
    let path = args.get(1).ok_or("missing package")?;
    let dir = script_dir(args, path);
    let db = load_db(&dir)?;
    let src = TextureSource::open(&dir)?;
    if src.index.is_none() {
        eprintln!("note: no MergedFileIndexCache.bin in {}; only inline mips are reachable", dir.display());
    }
    let pf = PackageFile::open(path)?;
    let pkg = pf.load()?;
    let pkg_name = Path::new(path).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let layout = package_layout(&pkg);
    let parse = |i: usize| -> Result<tempest_assets::object::Object, Box<dyn std::error::Error>> {
        match parse_export(&db, &pkg, layout, i, &pkg.export_data[i]) {
            Parsed::Object(o) if matches!(o.native, Native::Texture2D(_)) => Ok(o),
            _ => Err(format!("{} is not a decoded Texture2D", pkg.full_name(PackageIndex::export(i))).into()),
        }
    };
    let loc_text = |l: &MipLocation| match l {
        MipLocation::Inline => "inline".to_string(),
        MipLocation::Unused => "unused (stripped)".to_string(),
        MipLocation::Missing => "MISSING".to_string(),
        MipLocation::Cache { file, offset, size_on_disk, size } => format!("{file}.tfc @{offset} ({size_on_disk} stored, {size} bytes)"),
    };
    match sub {
        "info" => {
            let i = find_export(&pkg, args.get(2).ok_or("missing export")?)?;
            let o = parse(i)?;
            let info = texture_ops::texture_info(&db, &pkg, &pkg_name, &src, i, &o)?;
            println!("{}", pkg.full_name(PackageIndex::export(i)));
            println!("engine path: {}", info.path);
            println!("format:      {}{}", info.format_name, if info.format.is_none() { " (unsupported for export)" } else { "" });
            println!("size:        {}x{}{}", info.width, info.height, if info.lightmap { " (lightmap)" } else { "" });
            for m in &info.mips {
                println!("  mip {:2}  {:5}x{:<5} {}", m.index, m.width, m.height, loc_text(&m.location));
            }
        }
        "export" => {
            let i = find_export(&pkg, args.get(2).ok_or("missing export")?)?;
            let out = args.get(3).ok_or("missing output file")?;
            let mip = args.iter().position(|a| a == "--mip").and_then(|p| args.get(p + 1)).and_then(|v| v.parse().ok()).unwrap_or(0);
            let o = parse(i)?;
            let bytes = if out.to_ascii_lowercase().ends_with(".png") {
                texture_ops::export_png(&db, &pkg, &pkg_name, &src, i, &o, mip)?
            } else {
                texture_ops::export_dds(&db, &pkg, &pkg_name, &src, i, &o)?
            };
            std::fs::write(out, &bytes)?;
            eprintln!("wrote {} bytes to {out}", bytes.len());
        }
        "import" => {
            let i = find_export(&pkg, args.get(2).ok_or("missing export")?)?;
            let input = args.get(3).ok_or("missing input image")?;
            let out = args.iter().position(|a| a == "-o").and_then(|p| args.get(p + 1)).ok_or("missing -o <out.upk>")?;
            let mips = if args.iter().any(|a| a == "--no-mips") { Some(false) } else if args.iter().any(|a| a == "--mips") { Some(true) } else { None };
            let mut opts = texture_ops::ImportOptions { mips, format: None };
            if let Some(f) = args.iter().position(|a| a == "--format").and_then(|p| args.get(p + 1)) {
                opts.format = Some(tempest_assets::dds::PixelFormat::from_name(f).ok_or_else(|| format!("unknown pixel format {f}"))?);
            }
            let bytes = std::fs::read(input)?;
            let source = if input.to_ascii_lowercase().ends_with(".dds") {
                let d = tempest_assets::dds::read_dds(&bytes)?;
                texture_ops::ImportSource::Encoded { format: d.format, width: d.width, height: d.height, mips: d.mips }
            } else {
                let (w, h, rgba) = tempest_assets::dds::read_png(&bytes)?;
                texture_ops::ImportSource::Rgba { width: w, height: h, rgba }
            };
            let mut o = parse(i)?;
            let mut pkg = pkg;
            texture_ops::import(&db, &mut pkg, i, &mut o, source, &opts)?;
            save_with_export(&mut pkg, i, &o, out)?;
        }
        "export-all" => {
            let outdir = PathBuf::from(args.get(2).ok_or("missing output dir")?);
            let png = args.iter().any(|a| a == "--png");
            std::fs::create_dir_all(&outdir)?;
            let (mut ok, mut failed) = (0, 0);
            for i in 0..pkg.exports.len() {
                if !db.class_is_a(&pkg.class_name_of(PackageIndex::export(i)), "Texture2D") {
                    continue;
                }
                let Ok(o) = parse(i) else { continue };
                let name = pkg.object_path(PackageIndex::export(i)).replace(['/', '\\', ':'], "_");
                let r = if png {
                    texture_ops::export_png(&db, &pkg, &pkg_name, &src, i, &o, 0).map(|b| (b, format!("{name}.png")))
                } else {
                    texture_ops::export_dds(&db, &pkg, &pkg_name, &src, i, &o).map(|b| (b, format!("{name}.dds")))
                };
                match r {
                    Ok((bytes, file)) => {
                        std::fs::write(outdir.join(file), bytes)?;
                        ok += 1;
                    }
                    Err(e) => {
                        failed += 1;
                        if failed <= 10 {
                            eprintln!("{}: {e}", pkg.object_path(PackageIndex::export(i)));
                        }
                    }
                }
            }
            println!("exported {ok}, failed {failed}");
        }
        _ => return Err(format!("unknown texture subcommand {sub}").into()),
    }
    Ok(())
}

/// Store a rewritten export and save the package like the original was
/// stored (compressed and obscured, or plain).
fn save_with_export(pkg: &mut Package, i: usize, obj: &tempest_assets::object::Object, out: &str) -> Res {
    let none = tempest_assets::object::none_name(pkg);
    let data = tempest_assets::object::write_object(obj, none);
    pkg.set_export_data(i, data);
    let obscure = pkg.summary.compression_flags & tempest_upk::flags::compression::OBSCURED != 0;
    let compress = pkg.was_compressed;
    pkg.save(out, &WriteOptions { compress, obscure })?;
    eprintln!("wrote {out}");
    Ok(())
}

fn cmd_media(args: &[String], swf: bool) -> Res {
    use tempest_assets::media;
    use tempest_assets::object::Parsed;
    let sub = args[0].as_str();
    let path = &args[1];
    let dir = script_dir(args, path);
    let db = load_db(&dir)?;
    let pf = PackageFile::open(path)?;
    let mut pkg = pf.load()?;
    let layout = package_layout(&pkg);
    let i = find_export(&pkg, args.get(2).ok_or("missing export")?)?;
    let Parsed::Object(mut o) = parse_export(&db, &pkg, layout, i, &pkg.export_data[i]) else {
        return Err(format!("{} could not be parsed", pkg.full_name(PackageIndex::export(i))).into());
    };
    let file = args.get(3).ok_or("missing file argument")?;
    match sub {
        "export" => {
            let bytes = if swf { media::swf_export(&pkg, &o)? } else { media::sound_export(&pkg, &o)? };
            std::fs::write(file, &bytes)?;
            eprintln!("wrote {} bytes to {file}", bytes.len());
        }
        "import" => {
            let out = args.iter().position(|a| a == "-o").and_then(|p| args.get(p + 1)).ok_or("missing -o <out.upk>")?;
            let bytes = std::fs::read(file)?;
            if swf {
                media::swf_import(&mut pkg, &mut o, &bytes)?;
            } else {
                media::sound_import(&mut pkg, &mut o, &bytes)?;
            }
            save_with_export(&mut pkg, i, &o, out)?;
        }
        _ => return Err(format!("unknown subcommand {sub}").into()),
    }
    Ok(())
}

fn cmd_mesh(args: &[String]) -> Res {
    use tempest_assets::mesh_ops;
    use tempest_assets::object::{Native, Parsed};
    let sub = args[0].as_str();
    let path = &args[1];
    let dir = script_dir(args, path);
    let db = load_db(&dir)?;
    let pf = PackageFile::open(path)?;
    let pkg = pf.load()?;
    let layout = package_layout(&pkg);
    let i = find_export(&pkg, args.get(2).ok_or("missing export")?)?;
    let Parsed::Object(o) = parse_export(&db, &pkg, layout, i, &pkg.export_data[i]) else {
        return Err(format!("{} could not be parsed", pkg.full_name(PackageIndex::export(i))).into());
    };
    if !matches!(o.native, Native::StaticMesh(_) | Native::SkeletalMesh(_)) {
        return Err(format!("{} is not a decoded static or skeletal mesh", pkg.full_name(PackageIndex::export(i))).into());
    }
    let name = pkg.object_name(PackageIndex::export(i));
    match sub {
        "info" => {
            let mut out = String::new();
            let d = Dumper::new(&pkg);
            out.push_str(&d.export_line(i));
            out.push('\n');
            d.object(&o, &mut out);
            print!("{out}");
            for lod in 0..mesh_ops::lod_count(&o) {
                match mesh_ops::geometry(&pkg, &o, lod) {
                    Ok(g) => println!("  lod {lod} geometry: {} vertices, {} triangles, {} sections{}", g.positions.len(), g.indices.len() / 3, g.sections.len(), if g.bone_names.is_empty() { String::new() } else { format!(", {} bones", g.bone_names.len()) }),
                    Err(e) => println!("  lod {lod}: {e}"),
                }
            }
        }
        "export" => {
            let out = args.get(3).ok_or("missing output file")?;
            let lod = args.iter().position(|a| a == "--lod").and_then(|p| args.get(p + 1)).and_then(|v| v.parse().ok()).unwrap_or(0);
            let mut g = mesh_ops::geometry(&pkg, &o, lod)?;
            if args.iter().any(|a| a == "--no-skin") {
                g.bones.clear();
            }
            if !args.iter().any(|a| a == "--no-textures") && !out.to_ascii_lowercase().ends_with(".obj") {
                let src = tempest_assets::texcache::TextureSource::open(&dir)?;
                let pkg_name = Path::new(path).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                let mut cache = tempest_assets::material_ops::PackageCache::new(&db, &dir);
                cache.insert(&pkg_name, pkg.clone());
                for w in mesh_ops::attach_materials(&mut g, &mut cache, &src, &pkg_name) {
                    eprintln!("texture skipped: {w}");
                }
                eprintln!("{} materials, {} textures embedded", g.materials.len(), g.images.len());
            }
            let bytes = if out.to_ascii_lowercase().ends_with(".obj") { mesh_ops::write_obj(&name, &g).into_bytes() } else { mesh_ops::write_glb(&name, &g)? };
            std::fs::write(out, &bytes)?;
            eprintln!("wrote {} bytes to {out} ({} vertices, {} triangles)", bytes.len(), g.positions.len(), g.indices.len() / 3);
        }
        "import" => {
            let input = args.get(3).ok_or("missing input mesh file")?;
            let out = out_arg(args)?;
            let lod = args.iter().position(|a| a == "--lod").and_then(|p| args.get(p + 1)).and_then(|v| v.parse().ok()).unwrap_or(0);
            let bytes = std::fs::read(input)?;
            let mut o = o;
            let mut pkg = pkg;
            let report = tempest_assets::mesh_import::import(&pkg, &mut o, Path::new(input), &bytes, lod)?;
            for w in &report.warnings {
                eprintln!("warning: {w}");
            }
            eprintln!("imported {} vertices, {} triangles, {} sections{}", report.vertices, report.triangles, report.sections, if report.chunks > 0 { format!(", {} chunks", report.chunks) } else { String::new() });
            save_with_export(&mut pkg, i, &o, &out)?;
        }
        _ => return Err(format!("unknown mesh subcommand {sub}").into()),
    }
    Ok(())
}

fn cmd_native_debug(path: &str, what: &str, args: &[String]) -> Res {
    use tempest_assets::object::Parsed;
    use tempest_unrealscript::objects::read_object_header;
    let db = load_db(&script_dir(args, path))?;
    let pf = PackageFile::open(path)?;
    let pkg = pf.load()?;
    let layout = package_layout(&pkg);
    let i = find_export(&pkg, what)?;
    let data = &pkg.export_data[i];
    let (opts, _, class_name) = tempest_assets::object::header_options(&db, &pkg, i);
    let mut r = tempest_upk::Reader::new(data);
    let header = read_object_header(&mut r, &pkg, opts)?;
    let tail = &data[r.pos()..];
    println!("{}: header {} bytes, native {} bytes", pkg.full_name(PackageIndex::export(i)), r.pos(), tail.len());
    let props = match parse_export(&db, &pkg, layout, i, data) {
        Parsed::Object(o) => o.properties,
        _ => Vec::new(),
    };
    let _ = header;
    print!("{}", tempest_assets::object::native_debug(&db, &class_name, &props, &pkg, tail));
    let n = tail.len().min(96);
    let hex: Vec<String> = tail[..n].iter().map(|b| format!("{b:02x}")).collect();
    println!("head: {}", hex.join(" "));
    Ok(())
}

fn out_arg(args: &[String]) -> Res2<String> {
    args.iter().position(|a| a == "-o").and_then(|p| args.get(p + 1)).cloned().ok_or_else(|| "missing -o <out.upk>".into())
}

type Res2<T> = Result<T, Box<dyn std::error::Error>>;

fn cmd_prop(args: &[String]) -> Res {
    use tempest_assets::edit::Editor;
    let sub = args[0].as_str();
    let path = &args[1];
    let db = load_db(&script_dir(args, path))?;
    let mut ed = Editor::open(&db, path)?;
    let i = ed.find(args.get(2).ok_or("missing export")?)?;
    match sub {
        "get" => {
            let p = args.get(3).filter(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("");
            println!("{}", ed.get(i, p)?);
        }
        "set" => {
            let out = out_arg(args)?;
            let rest: Vec<&String> = args[3..].iter().take_while(|a| *a != "-o").collect();
            if rest.len() < 2 || !rest.len().is_multiple_of(2) {
                return Err("expected <Prop.Path> <value> pairs".into());
            }
            for pair in rest.chunks(2) {
                ed.set(i, pair[0], pair[1])?;
                eprintln!("{} = {}", pair[0], ed.get(i, pair[0])?);
            }
            ed.save(&out)?;
            eprintln!("wrote {out}");
        }
        _ => return Err(format!("unknown prop subcommand {sub}").into()),
    }
    Ok(())
}

fn cmd_obj(args: &[String]) -> Res {
    use tempest_assets::edit::Editor;
    let sub = args[0].as_str();
    let path = &args[1];
    let db = load_db(&script_dir(args, path))?;
    let out = out_arg(args)?;
    match sub {
        "rename" => {
            let mut ed = Editor::open(&db, path)?;
            let i = ed.find(args.get(2).ok_or("missing export")?)?;
            ed.rename(i, args.get(3).ok_or("missing new name")?)?;
            ed.save(&out)?;
        }
        "delete" => {
            let mut ed = Editor::open(&db, path)?;
            let targets: Vec<&String> = args[2..].iter().take_while(|a| *a != "-o").collect();
            for t in targets {
                let i = ed.find(t)?;
                let n = ed.delete(i)?;
                eprintln!("deleted {t} ({n} references cleared)");
            }
            ed.save(&out)?;
        }
        "copy" => {
            let dst_path = args.get(3).ok_or("missing destination package")?;
            let mut src = Editor::open(&db, path)?;
            let mut dst = Editor::open(&db, dst_path)?;
            let i = src.find(args.get(2).ok_or("missing export")?)?;
            let deep = !args.iter().any(|a| a == "--shallow");
            let outer = match args.iter().position(|a| a == "--outer").and_then(|p| args.get(p + 1)) {
                Some(o) => Some(PackageIndex::export(dst.find(o)?)),
                None if args.iter().any(|a| a == "--top") => Some(PackageIndex::NONE),
                None => None,
            };
            let (idx, warnings) = dst.copy_from(&mut src, i, outer, deep)?;
            for w in warnings {
                eprintln!("warning: {w}");
            }
            eprintln!("copied as export {} {}", idx.export_index().unwrap(), dst.pkg.full_name(idx));
            dst.save(&out)?;
        }
        _ => return Err(format!("unknown obj subcommand {sub}").into()),
    }
    eprintln!("wrote {out}");
    Ok(())
}

fn cmd_deps(path: &str, what: &str, args: &[String]) -> Res {
    use tempest_assets::edit::{referrers, Editor};
    use tempest_assets::object::Parsed;
    use tempest_assets::visit::references;
    let db = load_db(&script_dir(args, path))?;
    let mut ed = Editor::open(&db, path)?;
    let i = ed.find(what)?;
    println!("{}", ed.pkg.full_name(PackageIndex::export(i)));
    let e = ed.pkg.exports[i].clone();
    println!("  class {}  outer {}  archetype {}", ed.pkg.object_path(e.class), ed.pkg.object_path(e.outer), ed.pkg.object_path(e.archetype));
    let refs = match ed.parsed(i)? {
        Parsed::Object(o) => references(o),
        _ => Vec::new(),
    };
    println!("references ({}):", refs.len());
    for r in refs {
        println!("  {}{}", ed.pkg.full_name(r), if r.is_import() { "  (import)" } else { "" });
    }
    let back = referrers(&mut ed, i);
    println!("referenced by ({}):", back.len());
    for j in back {
        println!("  {}", ed.pkg.full_name(PackageIndex::export(j)));
    }
    Ok(())
}

fn cmd_mod_dir(args: &[String]) -> Res {
    let dir = PathBuf::from(&args[0]);
    let get = |flag: &str| args.iter().position(|a| a == flag).and_then(|p| args.get(p + 1)).cloned();
    let id = get("--id").ok_or("missing --id")?;
    let name = get("--name").unwrap_or_else(|| id.clone());
    let version = get("--version").unwrap_or_else(|| "1.0.0".into());
    let author = get("--author");
    let files: Vec<&String> = args[1..].iter().filter(|a| a.to_ascii_lowercase().ends_with(".upk") || a.to_ascii_lowercase().ends_with(".tfc")).collect();
    if files.is_empty() {
        return Err("no package files given".into());
    }
    std::fs::create_dir_all(dir.join("files"))?;
    let esc = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
    let mut manifest = format!("[mod]\nid = {}\nname = {}\nversion = {}\n", esc(&id), esc(&name), esc(&version));
    if let Some(a) = author {
        manifest.push_str(&format!("\n[[mod.authors]]\nname = {}\n", esc(&a)));
    }
    std::fs::write(dir.join("manifest.toml"), manifest)?;
    for f in files {
        let p = Path::new(f);
        let fname = p.file_name().ok_or("bad file name")?;
        std::fs::copy(p, dir.join("files").join(fname))?;
        eprintln!("added files/{}", fname.to_string_lossy());
    }
    eprintln!("mod folder ready: {}\n  package + sign it with: just mod-pack {} <out.tempest>", dir.display(), dir.display());
    Ok(())
}
