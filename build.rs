use std::{collections::HashSet, error::Error, fs::File, path::Path};

fn main() -> Result<(), Box<dyn Error>> {
    let root_dir = Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).to_path_buf();
    let out_dir = Path::new(&std::env::var("OUT_DIR").unwrap()).to_path_buf();

    let patch_dir = root_dir.join("patch");
    let svd_dir = out_dir.join("svd");
    let vendor_dir = root_dir.join("vendor");

    let supported_mcus = std::fs::read_dir(&vendor_dir)
        .unwrap()
        .filter_map(|res| res.ok())
        .filter_map(|de| {
            de.file_name()
                .to_str()
                .and_then(|s| s.strip_suffix(".atdf"))
                .map(|s| s.to_owned())
        })
        .collect::<HashSet<_>>();

    let features = std::env::vars()
        .filter_map(|(ref k, _)| {
            k.strip_prefix("CARGO_FEATURE_")
                .map(|f| f.to_lowercase())
                .map(|f| f.to_owned())
        })
        .collect::<HashSet<_>>();

    let mcus = if features.contains("all_mcus") {
        supported_mcus
    } else {
        let mcus = features
            .intersection(&supported_mcus)
            .cloned()
            .collect::<HashSet<_>>();
        if mcus.is_empty() {
            let mut supported = Vec::from_iter(supported_mcus);
            supported.sort_unstable();
            eprintln!("Supported MCUS:\n\t{}", supported.join("\n\t"));
            Err("at least one MCU must be enabled as a crate feature")?
        }
        mcus
    };

    std::fs::create_dir_all(&svd_dir).unwrap();
    for cpu in &mcus {
        // Generate SVD
        let svd_path = svd_dir.join(format!("{cpu}.svd"));
        generate_svd(&vendor_dir.join(format!("{cpu}.atdf")), &svd_path)?;

        // Patch SVD
        let cpu_patch = patch_dir.join(format!("{cpu}.yaml"));
        let svd = if cpu_patch.exists() {
            let svd_out = svdtools::patch::process_reader(
                File::open(&svd_path)?,
                &svdtools::patch::load_patch(&cpu_patch)?,
                &svdtools::patch::EncoderConfig::default(),
                &svdtools::patch::Config::default(),
            )
            .map_err(|e| format!("unable to apply patch {}: {e}", cpu_patch.display()))?;
            std::io::read_to_string(svd_out)?
        } else {
            std::fs::read_to_string(&svd_path)?
        };
        std::fs::write(svd_path.with_extension("svd.patched"), &svd)?;

        // SVD -> RS
        let cfg = svd2rust::Config {
            target: svd2rust::util::Target::None,
            make_mod: true,
            generic_mod: true,
            strict: true,
            log_level: Some("DEBUG".into()),
            ..Default::default()
        };

        let gen = svd2rust::generate(&svd, &cfg)?;
        std::fs::write(out_dir.join(format!("{cpu}.rs")), &gen.lib_rs)?;
    }

    todo!("testing...");
    Ok(())
}

fn generate_svd<P: AsRef<Path>>(atdf: &P, svd: &P) -> Result<(), Box<dyn Error>> {
    let atdf_file =
        File::open(atdf).map_err(|e| format!("unable to open {}: {e}", atdf.as_ref().display()))?;
    let chip = atdf2svd::atdf::parse(atdf_file, &HashSet::from_iter([])).map_err(|e| {
        let mut err = Vec::new();
        e.format(&mut err).unwrap();
        format!(
            "unable to parse {}: {}",
            atdf.as_ref().display(),
            String::from_utf8(err).unwrap()
        )
    })?;

    let svd_file = File::create(svd)
        .map_err(|e| format!("unable to create {}: {e}", svd.as_ref().display()))?;
    atdf2svd::svd::generate(&chip, svd_file).map_err(|e| {
        let mut err = Vec::new();
        e.format(&mut err).unwrap();
        format!(
            "unable to write {}: {}",
            svd.as_ref().display(),
            String::from_utf8(err).unwrap()
        )
    })?;

    Ok(())
}
