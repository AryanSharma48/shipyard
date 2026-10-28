use crate::builder::detector::{NodeProjectConfig, PackageManager};
use std::fmt::Write;

pub struct DockerfileGenerator;

impl DockerfileGenerator {
    /**
     * Generates a hardened, multi-stage Dockerfile from a [`NodeProjectConfig`].
     *
     * The generated Dockerfile follows security and caching best practices:
     * 1. Stage 1 (deps): Installs dependencies with cached lockfile
     * 2. Stage 2 (builder): Runs build script (if configured)
     * 3. Stage 3 (runner): Minimal alpine image, non-root `node` user, healthcheck, and exposed port 3000
     *
     * # Examples
     *
     * ```rust
     * use shipyard_api::builder::{DockerfileGenerator, NodeProjectConfig, PackageManager};
     *
     * let config = NodeProjectConfig {
     *     package_manager: PackageManager::Npm,
     *     install_command: "npm ci".to_string(),
     *     build_command: Some("npm run build".to_string()),
     *     start_command: "node server.js".to_string(),
     * };
     *
     * let dockerfile = DockerfileGenerator::generate(&config, Some("20-alpine"));
     * assert!(dockerfile.contains("FROM node:20-alpine AS deps"));
     * ```
     */
    pub fn generate(config: &NodeProjectConfig, node_version: Option<&str>) -> String {
        let version = node_version.unwrap_or("20-alpine");
        let (lockfile, pm_setup) = match config.package_manager {
            PackageManager::Npm => ("package-lock.json*", ""),
            PackageManager::Pnpm => (
                "pnpm-lock.yaml*",
                "RUN corepack enable && corepack prepare pnpm@latest --activate\n",
            ),
            PackageManager::Yarn => (
                "yarn.lock*",
                "RUN corepack enable && corepack prepare yarn@stable --activate\n",
            ),
        };

        let mut dockerfile = String::with_capacity(1024);

        // Stage 1: deps
        let _ = writeln!(
            dockerfile,
            "# Stage 1: Dependencies\n\
            FROM node:{version} AS deps\n\
            WORKDIR /app\n\
            {pm_setup}\
            COPY package.json {lockfile} ./\n\
            RUN {install_cmd}\n",
            install_cmd = config.install_command
        );

        // Stage 2: builder
        if let Some(build_cmd) = &config.build_command {
            let _ = writeln!(
                dockerfile,
                "# Stage 2: Builder\n\
                FROM node:{version} AS builder\n\
                WORKDIR /app\n\
                {pm_setup}\
                COPY --from=deps /app/node_modules ./node_modules\n\
                COPY . .\n\
                RUN {build_cmd}\n",
            );
        }

        // Stage 3: runner
        let _ = writeln!(
            dockerfile,
            "# Stage 3: Production Runner\n\
            FROM node:{version} AS runner\n\
            WORKDIR /app\n\
            ENV NODE_ENV=production\n\
            ENV PORT=3000\n\
            USER node",
        );

        if config.build_command.is_some() {
            dockerfile.push_str("COPY --from=builder --chown=node:node /app ./\n");
        } else {
            dockerfile.push_str(
                "COPY --from=deps --chown=node:node /app/node_modules ./node_modules\n\
                COPY --chown=node:node . ./\n",
            );
        }

        let formatted_cmd = Self::format_cmd(&config.start_command);
        let _ = writeln!(
            dockerfile,
            "EXPOSE 3000\n\
            HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \\\n  \
              CMD wget -qO- http://localhost:3000/health || exit 1\n\
            CMD {formatted_cmd}",
        );

        dockerfile
    }

    fn format_cmd(start_command: &str) -> String {
        let mut words = start_command.split_whitespace();
        let first = match words.next() {
            Some(w) => w,
            None => return "[\"node\", \"server.js\"]".to_string(),
        };

        let mut out = String::from("[\"");
        out.push_str(first);
        out.push('"');

        for word in words {
            out.push_str(", \"");
            out.push_str(word);
            out.push('"');
        }

        out.push(']');
        out
    }
}
