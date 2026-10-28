pub mod detector;
pub mod docker;
pub mod docker_template;
pub mod worker;

pub use detector::{NodeDetector, NodeProjectConfig, PackageManager};
pub use docker::{BuildResult, ContainerRunner, LocalDockerRunner, MockContainerRunner};
pub use docker_template::DockerfileGenerator;
pub use worker::BuildWorker;
