#[cfg(test)]
#[path = "../../../tests/unit/extensions/solve/config_test.rs"]
mod config_test;

extern crate serde_json;

use serde::Deserialize;
use std::io::{BufReader, Read};
use std::sync::Arc;
use vrp_core::models::Problem;
use vrp_core::solver::mutation::*;
use vrp_core::solver::Builder;

#[derive(Clone, Deserialize, Debug)]
pub struct Config {
    population: Option<PopulationConfig>,
    mutation: Option<MutationConfig>,
    termination: Option<TerminationConfig>,
    logging: Option<LoggingConfig>,
}

#[derive(Clone, Deserialize, Debug)]
#[serde(tag = "type")]
pub enum MutationConfig {
    #[serde(rename(deserialize = "ruin-recreate"))]
    RuinRecreate {
        /// Ruin methods.
        ruins: Vec<ConfigRuinGroup>,
        /// Recreate methods.
        recreates: Vec<RecreateMethod>,
    },
}

#[derive(Clone, Deserialize, Debug)]
pub struct ConfigRuinGroup {
    methods: Vec<RuinMethod>,
    weight: usize,
}

/// Specifies ruin methods with their probability weight and specific parameters.
#[derive(Clone, Deserialize, Debug)]
#[serde(tag = "type")]
pub enum RuinMethod {
    #[serde(rename(deserialize = "adjusted-string"))]
    AdjustedString { probability: f64, lmax: usize, cavg: usize, alpha: f64 },
    #[serde(rename(deserialize = "neighbour"))]
    Neighbour { probability: f64, min: usize, max: usize, threshold: f64 },
    #[serde(rename(deserialize = "random-job"))]
    RandomJob { probability: f64, min: usize, max: usize, threshold: f64 },
    #[serde(rename(deserialize = "random-route"))]
    RandomRoute { probability: f64, min: usize, max: usize, threshold: f64 },
    #[serde(rename(deserialize = "worst-job"))]
    WorstJob { probability: f64, min: usize, max: usize, threshold: f64, skip: usize },
    #[serde(rename(deserialize = "cluster"))]
    Cluster { probability: f64, min: usize, max: usize, threshold: f64, cmin: usize, cmax: usize },
}

/// Specifies recreate methods with their probability weight and specific parameters.
#[derive(Clone, Deserialize, Debug)]
#[serde(tag = "type")]
pub enum RecreateMethod {
    #[serde(rename(deserialize = "cheapest"))]
    Cheapest { weight: usize },
    #[serde(rename(deserialize = "regret"))]
    Regret { weight: usize, start: usize, end: usize },
    #[serde(rename(deserialize = "blinks"))]
    Blinks { weight: usize },
    #[serde(rename(deserialize = "gaps"))]
    Gaps { weight: usize, min: usize },
    #[serde(rename(deserialize = "nearest"))]
    Nearest { weight: usize },
}

#[derive(Clone, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PopulationConfig {
    initial_methods: Option<Vec<RecreateMethod>>,
    initial_size: Option<usize>,
    population_size: Option<usize>,
    offspring_size: Option<usize>,
    elite_size: Option<usize>,
}

#[derive(Clone, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TerminationConfig {
    max_time: Option<usize>,
    max_generations: Option<usize>,
    variation: Option<VariationConfig>,
}

#[derive(Clone, Deserialize, Debug)]
pub struct VariationConfig {
    sample: usize,
    cv: f64,
}

#[derive(Clone, Deserialize, Debug)]
pub struct LoggingConfig {
    enabled: bool,
}

fn configure_from_population(mut builder: Builder, population_config: &Option<PopulationConfig>) -> Builder {
    if let Some(config) = population_config {
        if let Some(methods) = &config.initial_methods {
            builder =
                builder.with_initial_methods(methods.iter().map(|method| create_recreate_method(method)).collect());
        }

        if let Some(initial_size) = &config.initial_size {
            builder = builder.with_initial_size(*initial_size);
        }

        if let Some(population_size) = &config.population_size {
            builder = builder.with_population_size(*population_size);
        }

        if let Some(elite_size) = &config.elite_size {
            builder = builder.with_elite_size(*elite_size);
        }

        if let Some(offspring_size) = &config.offspring_size {
            builder = builder.with_offspring_size(*offspring_size);
        }
    }

    builder
}

fn configure_from_mutation(mut builder: Builder, mutation_config: &Option<MutationConfig>) -> Builder {
    if let Some(config) = mutation_config {
        let MutationConfig::RuinRecreate { ruins, recreates } = config;
        let problem = builder.config.problem.clone();
        builder = builder.with_mutation(Box::new(RuinAndRecreateMutation::new(
            Box::new(CompositeRecreate::new(recreates.iter().map(|r| create_recreate_method(r)).collect())),
            Box::new(CompositeRuin::new(ruins.iter().map(|g| create_ruin_group(&problem, g)).collect())),
        )));
    }

    builder
}

fn configure_from_termination(mut builder: Builder, termination_config: &Option<TerminationConfig>) -> Builder {
    if let Some(config) = termination_config {
        builder = builder.with_max_time(config.max_time);
        builder = builder.with_max_generations(config.max_generations);
        builder = builder.with_cost_variation(config.variation.as_ref().map(|v| (v.sample, v.cv)));
    }

    builder
}

fn create_recreate_method(method: &RecreateMethod) -> (Box<dyn Recreate>, usize) {
    match method {
        RecreateMethod::Cheapest { weight } => (Box::new(RecreateWithCheapest::default()), *weight),
        RecreateMethod::Regret { weight, start, end } => (Box::new(RecreateWithRegret::new(*start, *end)), *weight),
        RecreateMethod::Blinks { weight } => (Box::new(RecreateWithBlinks::<i32>::default()), *weight),
        RecreateMethod::Gaps { weight, min } => (Box::new(RecreateWithGaps::new(*min)), *weight),
        RecreateMethod::Nearest { weight } => (Box::new(RecreateWithNearestNeighbor::default()), *weight),
    }
}

fn create_ruin_group(problem: &Arc<Problem>, group: &ConfigRuinGroup) -> RuinGroup {
    (group.methods.iter().map(|r| create_ruin_method(problem, r)).collect(), group.weight)
}

fn create_ruin_method(problem: &Arc<Problem>, method: &RuinMethod) -> (Arc<dyn Ruin>, f64) {
    match method {
        RuinMethod::AdjustedString { probability, lmax, cavg, alpha } => {
            (Arc::new(AdjustedStringRemoval::new(*lmax, *cavg, *alpha)), *probability)
        }
        RuinMethod::Neighbour { probability, min, max, threshold } => {
            (Arc::new(NeighbourRemoval::new(JobRemovalLimit::new(*min, *max, *threshold))), *probability)
        }
        RuinMethod::RandomJob { probability, min, max, threshold } => {
            (Arc::new(RandomJobRemoval::new(JobRemovalLimit::new(*min, *max, *threshold))), *probability)
        }
        RuinMethod::RandomRoute { probability, min, max, threshold } => {
            (Arc::new(RandomRouteRemoval::new(*min, *max, *threshold)), *probability)
        }
        RuinMethod::WorstJob { probability, min, max, threshold, skip: worst_skip } => {
            (Arc::new(WorstJobRemoval::new(*worst_skip, JobRemovalLimit::new(*min, *max, *threshold))), *probability)
        }
        RuinMethod::Cluster { probability, min, max, threshold, cmin, cmax } => (
            Arc::new(ClusterRemoval::new(problem.clone(), *cmin..*cmax, JobRemovalLimit::new(*min, *max, *threshold))),
            *probability,
        ),
    }
}

fn configure_from_logging(builder: Builder, logging_config: &Option<LoggingConfig>) -> Builder {
    let is_enabled = logging_config.as_ref().map(|l| l.enabled).unwrap_or(true);
    if !is_enabled {
        builder.with_logger(Arc::new(|_| {}))
    } else {
        builder
    }
}

/// Reads config from reader.
pub fn read_config<R: Read>(reader: BufReader<R>) -> Result<Config, String> {
    serde_json::from_reader(reader).map_err(|err| format!("cannot deserialize config: '{}'", err))
}

/// Creates a solver `Builder` from config file.
pub fn create_builder_from_config_file<R: Read>(
    problem: Arc<Problem>,
    reader: BufReader<R>,
) -> Result<Builder, String> {
    read_config(reader).and_then(|config| create_builder_from_config(problem, &config))
}

/// Creates a solver `Builder` from config.
pub fn create_builder_from_config(problem: Arc<Problem>, config: &Config) -> Result<Builder, String> {
    let mut builder = Builder::new(problem);

    builder = configure_from_logging(builder, &config.logging);
    builder = configure_from_population(builder, &config.population);
    builder = configure_from_mutation(builder, &config.mutation);
    builder = configure_from_termination(builder, &config.termination);

    Ok(builder)
}
