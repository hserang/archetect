mod cli;

use archetect::config::{
    AnswerConfig, AnswerConfigError, AnswerInfo, Catalog, CatalogEntry, CatalogError, CATALOG_FILE_NAME,
};
use archetect::input::select_from_catalog;
use archetect::system::SystemError;
use archetect::util::{Source, SourceError};
use archetect::RenderError;
use archetect::{self, ArchetectError, ArchetypeError};
use clap::{ArgMatches, Shell};
//use indoc::indoc;
use archetect::requirements::RequirementsError;
use linked_hash_map::LinkedHashMap;
use log::{error, info, warn};
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;

fn main() {
    let matches = cli::get_matches().get_matches();

    cli::configure(&matches);

    match execute(matches) {
        Ok(()) => (),
        Err(error) => handle_archetect_error(error),
    }
}

fn execute(matches: ArgMatches) -> Result<(), ArchetectError> {
    let mut archetect = archetect::Archetect::builder()
        .with_offline(matches.is_present("offline"))
        .build()?;

    let mut answers = LinkedHashMap::new();

    if let Ok(user_answers) = AnswerConfig::load(archetect.layout().answers_config()) {
        for (identifier, answer_info) in user_answers.answers() {
            answers.insert(identifier.to_owned(), answer_info.clone());
        }
    }

    if let Some(matches) = matches.values_of("answer-file") {
        for answer_file in matches {
            match AnswerConfig::load(answer_file) {
                Ok(answer_config) => {
                    for (identifier, answer_info) in answer_config.answers() {
                        answers.insert(identifier.to_owned(), answer_info.clone());
                    }
                }
                Err(cause) => {
                    return Err(ArchetectError::AnswerConfigError {
                        source: answer_file.to_owned(),
                        cause,
                    });
                }
            }
        }
    }

    if let Some(matches) = matches.values_of("answer") {
        for (identifier, answer_info) in matches.map(|m| AnswerInfo::parse(m).unwrap()) {
            answers.insert(identifier, answer_info);
        }
    }

    if let Some(matches) = matches.values_of("switches") {
        for switch in matches {
            archetect.enable_switch(switch);
        }
    }

    if let Some(matches) = matches.subcommand_matches("cache") {
        let git_cache = archetect.layout().git_cache_dir();
        if let Some(_sub_matches) = matches.subcommand_matches("clear") {
            fs::remove_dir_all(&git_cache).expect("Error deleting archetect cache");
        }
    }

    if let Some(matches) = matches.subcommand_matches("completions") {
        match matches.subcommand() {
            ("fish", Some(_)) => {
                cli::get_matches().gen_completions_to("archetect", Shell::Fish, &mut std::io::stdout())
            }
            ("bash", Some(_)) => {
                cli::get_matches().gen_completions_to("archetect", Shell::Bash, &mut std::io::stdout())
            }
            ("powershell", Some(_)) => {
                cli::get_matches().gen_completions_to("archetect", Shell::PowerShell, &mut std::io::stdout())
            }
            ("zsh", Some(_)) => cli::get_matches().gen_completions_to("archetect", Shell::Zsh, &mut std::io::stdout()),
            (&_, _) => warn!("Unsupported Shell"),
        }
    }

    if let Some(matches) = matches.subcommand_matches("system") {
        if let Some(matches) = matches.subcommand_matches("layout") {
            match matches.subcommand() {
                ("git", Some(_)) => println!("{}", archetect.layout().git_cache_dir().display()),
                ("http", Some(_)) => println!("{}", archetect.layout().http_cache_dir().display()),
                ("answers", Some(_)) => println!("{}", archetect.layout().answers_config().display()),
                ("catalogs", Some(_)) => println!("{}", archetect.layout().catalog_cache_dir().display()),
                ("config", Some(_)) => println!("{}", archetect.layout().configs_dir().display()),
                _ => println!("{}", archetect.layout()),
            }
        }
    }

    if let Some(matches) = matches.subcommand_matches("render") {
        let source = matches.value_of("source").unwrap();
        let destination = PathBuf::from_str(matches.value_of("destination").unwrap()).unwrap();

        let archetype = archetect.load_archetype(source, None)?;

        if let Ok(answer_config) = AnswerConfig::load(destination.clone()) {
            for (identifier, answer_info) in answer_config.answers() {
                answers.insert(identifier.to_owned(), answer_info.clone());
            }
        }
        archetype.execute_script(&archetect, &destination, &answers)?;
    }

    if let Some(matches) = matches.subcommand_matches("catalog") {
        let default_source = archetect.layout().catalog().to_str().map(|s| s.to_owned()).unwrap();
        let source = matches.value_of("source").unwrap_or_else(|| &default_source);
        let source = Source::detect(&archetect, source, None)?;
        let mut local_path = source.local_path().to_owned();
        if local_path.is_dir() {
            local_path.push(CATALOG_FILE_NAME);
        }

        if let Some(_matches) = matches.subcommand_matches("clear") {
            //            if you_are_sure(format!("Are you sure you want to clear the catalog at '{}'?", local_path.to_str().unwrap()).as_str()) {
            //                let catalog = Catalog::new();
            //                catalog.save_to_file(local_path)?;
            //                info!("Catalog at '{}' cleared!", local_path.to_str().unwrap());
            //            }
        } else if let Some(_matches) = matches.subcommand_matches("add") {
        } else {
            if source.local_path().exists() {
                let catalog_file = source.local_path();
                let catalog_source = Source::detect(&archetect, catalog_file.to_str().unwrap(), None)?;
                let catalog = Catalog::load(source.clone())?;

                let catalog_entry = select_from_catalog(&archetect, &catalog, &catalog_source)?;

                match catalog_entry {
                    CatalogEntry::Archetype { description: _, source } => {
                        let destination = PathBuf::from_str(matches.value_of("destination").unwrap()).unwrap();

                        let archetype = archetect.load_archetype(&source, None)?;

                        if let Ok(answer_config) = AnswerConfig::load(destination.clone()) {
                            for (identifier, answer_info) in answer_config.answers() {
                                if !answers.contains_key(identifier) {
                                    answers.insert(identifier.to_owned(), answer_info.clone());
                                }
                            }
                        }
                        archetype.execute_script(&archetect, &destination, &answers)?;
                        return Ok(());
                    }
                    _ => unreachable!(),
                }
            } else {
                info!("There are no items registered in your catalog. Try registering one first.");
            }
        }
    }

    Ok(())
}

fn handle_archetect_error(error: ArchetectError) {
    match error {
        ArchetectError::SourceError(error) => handle_source_error(error),
        ArchetectError::ArchetypeError(error) => handle_archetype_error(error),
        ArchetectError::GenericError(error) => error!("Archetect Error: {}", error),
        ArchetectError::RenderError(error) => handle_render_error(error),
        ArchetectError::SystemError(error) => handle_system_error(error),
        ArchetectError::CatalogError(error) => handle_catalog_error(error),
        ArchetectError::IoError(error) => handle_io_error(error),
        ArchetectError::AnswerConfigError { source, cause } => handle_answer_config_error(source, cause),
    }
}

fn handle_answer_config_error(source: String, error: AnswerConfigError) {
    match error {
        AnswerConfigError::MissingError => {
            error!("{} does not exist.", source);
        }
        AnswerConfigError::ParseError(cause) => {
            error!("Error parsing {}: {}", source, cause);
        }
    }
}

fn handle_io_error(error: std::io::Error) {
    error!("IO Error: {}", error);
}

fn handle_requirements_error(path: String, error: RequirementsError) {
    match error {
        RequirementsError::DeserializationError { path, cause } => {
            error!("Error reading {}:\n{}", path.display(), cause);
        }
        RequirementsError::ArchetectVersion(version, requirements) => {
            error!(
                "'{}' requires features that are unavailable in this version of \
                    Archetect.  Archetect {} is required, but you are on Archetect {}.  \
                    Try upgrading to the latest available version.",
                path, requirements, version
            );
        }
        RequirementsError::IoError(cause) => {
            error!("Error reading requirements: {}", cause);
        }
    }
}

fn handle_archetype_error(error: ArchetypeError) {
    match error {
        ArchetypeError::ArchetypeInvalid => panic!(),
        ArchetypeError::InvalidAnswersConfig => panic!(),
        ArchetypeError::RenderError(error) => handle_render_error(error),
        ArchetypeError::ArchetypeSaveFailed => {}
        ArchetypeError::SourceError(error) => handle_source_error(error),
        ArchetypeError::IoError(error) => handle_io_error(error),
        ArchetypeError::YamlError { path, cause } => {
            error!("Error reading {}:\n{}", path.display(), cause);
        }
    }
}

fn handle_source_error(error: SourceError) {
    match error {
        SourceError::SourceInvalidEncoding(source) => error!("\"{}\" is not valid UTF-8", source),
        SourceError::SourceNotFound(source) => error!("\"{}\" does not exist", source),
        SourceError::SourceUnsupported(source) => error!("\"{}\" is not a supported archetype path", source),
        SourceError::SourceInvalidPath(source) => error!("\"{}\" is not a valid archetype path", source),
        SourceError::OfflineAndNotCached(source) => error!(
            "\"{}\" is not cached locally and cannot be cloned in offline mode",
            source
        ),
        SourceError::RemoteSourceError(err) => error!("Remote Source Error\n{}", err),
        SourceError::IoError(err) => error!("IO Error: {}", err),
        SourceError::RequirementsError { path, cause } => {
            handle_requirements_error(path, cause);
        }
    };
}

fn handle_system_error(error: SystemError) {
    match error {
        SystemError::GenericError(error) => error!("System Error: {}", error),
        SystemError::IOError { error, message: _ } => error!("{}", error.to_string()),
    }
}

fn handle_render_error(error: RenderError) {
    match error {
        RenderError::FileRenderError {
            source,
            error,
            message: _,
        } => {
            if let Some(cause) = error.source() {
                error!("{} in template \"{}\"", cause, source.display());
            } else {
                error!("Error rendering template \"{}\"\n\n{}", source.display(), error);
            }
        }
        RenderError::FileRenderIOError {
            source,
            error,
            message: _,
        } => {
            error!("IO Error: {} in template \"{}\"", error, source.display());
        }
        RenderError::PathRenderError {
            source,
            error,
            message: _,
        } => {
            if let Some(cause) = error.source() {
                error!("{} in path \"{}\"", cause, source.display());
            } else {
                error!("Error rendering path name \"{}\"\n\n{:?}", source.display(), error);
            }
        }
        RenderError::StringRenderError {
            source,
            error: _,
            message,
        } => {
            error!("IO Error: {} in \"{}\"", message, source);
        }
        RenderError::IOError { error: _, message } => {
            error!("Unexpected IO Error:\n{}", message);
        }
        RenderError::InvalidPathCharacters { source } => {
            error!("Unable read path '{}' as a UTF-8 template", source.display());
        }
    }
}

fn handle_catalog_error(error: CatalogError) {
    match error {
        CatalogError::EmptyCatalog => error!("Empty Catalog"),
        CatalogError::EmptyGroup => error!("Empty Catalog Group"),
        CatalogError::SourceError(error) => error!("Catalog Source Error: {:?}", error),
        CatalogError::NotFound(error) => error!("Catalog not found: {}", error.to_str().unwrap()),
        CatalogError::IOError(error) => error!("Catalog IO Error: {}", error),
        CatalogError::YamlError(error) => error!("Catalog YAML Read Error: {}", error),
    }
}
