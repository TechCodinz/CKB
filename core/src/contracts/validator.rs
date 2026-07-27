//! Cross-Service API Contract Validator
//! Parses OpenAPI/JSON schema endpoint definitions and validates cross-service contracts

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiEndpoint {
    pub service_name: String,
    pub path: String,
    pub method: String,
    pub request_schema: Option<serde_json::Value>,
    pub response_schema: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractViolation {
    pub consumer_service: String,
    pub provider_service: String,
    pub endpoint: String,
    pub violation_kind: ContractViolationKind,
    pub description: String,
    pub severity: ContractSeverity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContractViolationKind {
    MissingEndpoint,
    SchemaFieldRemoved,
    SchemaTypeMismatch,
    MethodMismatch,
    BreakingChange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContractSeverity {
    Critical,
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractValidationReport {
    pub services_analyzed: usize,
    pub endpoints_checked: usize,
    pub violations: Vec<ContractViolation>,
    pub is_contract_safe: bool,
}

pub struct ApiContractValidator;

impl ApiContractValidator {
    /// Parse an OpenAPI JSON spec and extract endpoints for a service
    pub fn parse_openapi_spec(service_name: &str, spec_json: &str) -> anyhow::Result<Vec<ApiEndpoint>> {
        let spec: serde_json::Value = serde_json::from_str(spec_json)?;
        let paths = spec.get("paths").and_then(|p| p.as_object()).cloned().unwrap_or_default();

        let mut endpoints = Vec::new();
        for (path, methods) in &paths {
            if let Some(methods_obj) = methods.as_object() {
                for (method, operation) in methods_obj {
                    let req_schema = operation
                        .get("requestBody")
                        .and_then(|rb| rb.get("content"))
                        .and_then(|c| c.get("application/json"))
                        .and_then(|j| j.get("schema"))
                        .cloned();

                    let resp_schema = operation
                        .get("responses")
                        .and_then(|r| r.get("200"))
                        .and_then(|r| r.get("content"))
                        .and_then(|c| c.get("application/json"))
                        .and_then(|j| j.get("schema"))
                        .cloned();

                    endpoints.push(ApiEndpoint {
                        service_name: service_name.to_string(),
                        path: path.clone(),
                        method: method.to_uppercase(),
                        request_schema: req_schema,
                        response_schema: resp_schema,
                    });
                }
            }
        }

        Ok(endpoints)
    }

    /// Compare a consumer's expected contract against a provider's actual spec and report violations
    pub fn validate(
        consumer_endpoints: &[ApiEndpoint],
        provider_endpoints: &[ApiEndpoint],
    ) -> ContractValidationReport {
        let provider_map: HashMap<String, &ApiEndpoint> = provider_endpoints
            .iter()
            .map(|e| (format!("{}:{}", e.method, e.path), e))
            .collect();

        let mut violations = Vec::new();

        for consumer_ep in consumer_endpoints {
            let key = format!("{}:{}", consumer_ep.method, consumer_ep.path);

            match provider_map.get(&key) {
                None => violations.push(ContractViolation {
                    consumer_service: consumer_ep.service_name.clone(),
                    provider_service: provider_endpoints.first()
                        .map(|e| e.service_name.clone())
                        .unwrap_or_default(),
                    endpoint: format!("{} {}", consumer_ep.method, consumer_ep.path),
                    violation_kind: ContractViolationKind::MissingEndpoint,
                    description: format!(
                        "Consumer expects endpoint `{} {}` which does not exist in provider spec.",
                        consumer_ep.method, consumer_ep.path
                    ),
                    severity: ContractSeverity::Critical,
                }),
                Some(provider_ep) => {
                    // Check for schema field removal
                    if let (Some(cons_schema), Some(prov_schema)) =
                        (&consumer_ep.request_schema, &provider_ep.request_schema)
                    {
                        if let Some(violated_field) = Self::find_missing_field(cons_schema, prov_schema) {
                            violations.push(ContractViolation {
                                consumer_service: consumer_ep.service_name.clone(),
                                provider_service: provider_ep.service_name.clone(),
                                endpoint: format!("{} {}", consumer_ep.method, consumer_ep.path),
                                violation_kind: ContractViolationKind::SchemaFieldRemoved,
                                description: format!(
                                    "Required field `{}` present in consumer schema is missing in provider.",
                                    violated_field
                                ),
                                severity: ContractSeverity::High,
                            });
                        }
                    }
                }
            }
        }

        let is_safe = violations.is_empty();

        ContractValidationReport {
            services_analyzed: 2,
            endpoints_checked: consumer_endpoints.len(),
            violations,
            is_contract_safe: is_safe,
        }
    }

    /// Find the first field present in `consumer_schema` but absent in `provider_schema`
    fn find_missing_field(consumer: &serde_json::Value, provider: &serde_json::Value) -> Option<String> {
        let c_props = consumer.get("properties")?.as_object()?;
        let p_props = provider.get("properties").and_then(|p| p.as_object());

        if let Some(p) = p_props {
            for key in c_props.keys() {
                if !p.contains_key(key) {
                    return Some(key.clone());
                }
            }
        }

        None
    }
}
