//! Service discovery functionality for LM Studio

use crate::error::{LMStudioError, Result};
use crate::logger::Logger;
use crate::{LM_STUDIO_API_HOSTS, LM_STUDIO_API_PORTS};
use reqwest::Client;
use std::time::Duration;

/// Service discovery for finding running LM Studio instances
#[derive(Clone)]
pub struct Discovery {
    logger: Logger,
    client: Client,
}

impl Discovery {
    /// Create a new discovery instance
    pub fn new(logger: Logger) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap_or_default();

        Self { logger, client }
    }

    /// Discover running LM Studio instances
    pub async fn discover_instances(&self) -> Vec<String> {
        let mut instances = Vec::new();

        for &host in LM_STUDIO_API_HOSTS {
            for &port in LM_STUDIO_API_PORTS {
                let api_host = format!("http://{}:{}", host, port);
                if self.check_instance(&api_host).await {
                    instances.push(api_host);
                }
            }
        }

        instances
    }

    /// Check if a specific instance is running
    pub async fn check_instance(&self, api_host: &str) -> bool {
        let health_url = format!("{}/health", api_host);
        
        match self.client.get(&health_url).send().await {
            Ok(response) => {
                let is_ok = response.status().is_success();
                if is_ok {
                    self.logger.debug(&format!("Found LM Studio instance at: {}", api_host));
                } else {
                    self.logger.debug(&format!("Instance at {} returned status: {}", api_host, response.status()));
                }
                is_ok
            }
            Err(e) => {
                self.logger.debug(&format!("Failed to connect to {}: {}", api_host, e));
                false
            }
        }
    }

    /// Find the first available LM Studio instance
    pub async fn find_first_available(&self) -> Result<String> {
        let instances = self.discover_instances().await;
        
        if instances.is_empty() {
            return Err(LMStudioError::service_unavailable(
                "No running LM Studio instances found"
            ));
        }

        Ok(instances[0].clone())
    }

    /// Get the default API host (first host and port combination)
    pub fn default_api_host() -> String {
        format!("http://{}:{}", LM_STUDIO_API_HOSTS[0], LM_STUDIO_API_PORTS[0])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logger::LogLevel;

    #[test]
    fn test_default_api_host() {
        let host = Discovery::default_api_host();
        assert_eq!(host, "http://localhost:1234");
    }

    #[tokio::test]
    async fn test_discovery_creation() {
        let logger = Logger::new(LogLevel::Debug);
        let discovery = Discovery::new(logger);
        
        // This should not panic
        let _instances = discovery.discover_instances().await;
    }
}
