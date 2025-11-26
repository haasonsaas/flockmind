use flockmind::auth::certs::CaCertificate;
use flockmind::auth::enrollment::*;
use tempfile::TempDir;

fn create_test_manager() -> EnrollmentManager {
    let ca = CaCertificate::generate("test-cluster").unwrap();
    EnrollmentManager::new("test-cluster".to_string(), ca)
}

#[test]
fn test_enrollment_manager_new() {
    let manager = create_test_manager();
    assert_eq!(manager.cluster_id(), "test-cluster");
    assert!(!manager.ca_cert_pem().is_empty());
}

#[test]
fn test_load_or_create_new() {
    let temp_dir = TempDir::new().unwrap();
    let manager = EnrollmentManager::load_or_create(temp_dir.path(), "new-cluster").unwrap();

    assert_eq!(manager.cluster_id(), "new-cluster");
    assert!(temp_dir.path().join("ca.crt").exists());
    assert!(temp_dir.path().join("ca.key").exists());
}

#[test]
fn test_load_or_create_existing() {
    let temp_dir = TempDir::new().unwrap();

    let manager1 = EnrollmentManager::load_or_create(temp_dir.path(), "existing-cluster").unwrap();
    let cert_pem = manager1.ca_cert_pem().to_string();

    let manager2 = EnrollmentManager::load_or_create(temp_dir.path(), "existing-cluster").unwrap();
    assert_eq!(manager2.ca_cert_pem(), cert_pem);
}

#[test]
fn test_generate_token() {
    let manager = create_test_manager();
    let token = manager.generate_token(24, vec!["gpu".to_string()]);

    assert!(!token.token.is_empty());
    assert_eq!(token.cluster_id, "test-cluster");
    assert_eq!(token.allowed_tags, vec!["gpu".to_string()]);
}

#[test]
fn test_enroll_success() {
    let manager = create_test_manager();
    let token = manager.generate_token(24, vec![]);

    let req = EnrollmentRequest {
        token: token.token,
        node_id: "node-1".to_string(),
        hostname: "host1".to_string(),
        hostnames: vec!["localhost".to_string()],
        ips: vec!["127.0.0.1".to_string()],
        tags: vec!["dev".to_string()],
    };

    let resp = manager.enroll(req).unwrap();
    assert_eq!(resp.node_id, "node-1");
    assert_eq!(resp.cluster_id, "test-cluster");
    assert!(!resp.node_cert_pem.is_empty());
    assert!(!resp.node_key_pem.is_empty());
    assert!(!resp.ca_cert_pem.is_empty());
}

#[test]
fn test_enroll_invalid_token() {
    let manager = create_test_manager();

    let req = EnrollmentRequest {
        token: "invalid-token".to_string(),
        node_id: "node-1".to_string(),
        hostname: "host1".to_string(),
        hostnames: vec![],
        ips: vec![],
        tags: vec![],
    };

    let result = manager.enroll(req);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Invalid enrollment token"));
}

#[test]
fn test_enroll_token_consumed() {
    let manager = create_test_manager();
    let token = manager.generate_token(24, vec![]);
    let token_str = token.token.clone();

    let req1 = EnrollmentRequest {
        token: token_str.clone(),
        node_id: "node-1".to_string(),
        hostname: "host1".to_string(),
        hostnames: vec![],
        ips: vec![],
        tags: vec![],
    };

    manager.enroll(req1).unwrap();

    let req2 = EnrollmentRequest {
        token: token_str,
        node_id: "node-2".to_string(),
        hostname: "host2".to_string(),
        hostnames: vec![],
        ips: vec![],
        tags: vec![],
    };

    let result = manager.enroll(req2);
    assert!(result.is_err());
}

#[test]
fn test_enroll_tag_restriction() {
    let manager = create_test_manager();
    let token = manager.generate_token(24, vec!["gpu".to_string()]);

    let req = EnrollmentRequest {
        token: token.token,
        node_id: "node-1".to_string(),
        hostname: "host1".to_string(),
        hostnames: vec![],
        ips: vec![],
        tags: vec!["cpu".to_string()],
    };

    let result = manager.enroll(req);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not in allowed tags"));
}

#[test]
fn test_enroll_tag_restriction_success() {
    let manager = create_test_manager();
    let token = manager.generate_token(24, vec!["gpu".to_string(), "dev".to_string()]);

    let req = EnrollmentRequest {
        token: token.token,
        node_id: "node-1".to_string(),
        hostname: "host1".to_string(),
        hostnames: vec![],
        ips: vec![],
        tags: vec!["gpu".to_string()],
    };

    let result = manager.enroll(req);
    assert!(result.is_ok());
}

#[test]
fn test_register_enrolled_node() {
    let manager = create_test_manager();

    assert!(!manager.is_enrolled("node-1"));

    manager.register_enrolled_node(
        "node-1".to_string(),
        "host1".to_string(),
        "127.0.0.1:9000".to_string(),
        vec!["gpu".to_string()],
    );

    assert!(manager.is_enrolled("node-1"));
    assert!(!manager.is_enrolled("node-2"));
}

#[test]
fn test_get_enrolled_nodes() {
    let manager = create_test_manager();

    manager.register_enrolled_node(
        "node-1".to_string(),
        "host1".to_string(),
        "127.0.0.1:9000".to_string(),
        vec![],
    );
    manager.register_enrolled_node(
        "node-2".to_string(),
        "host2".to_string(),
        "127.0.0.1:9001".to_string(),
        vec![],
    );

    let nodes = manager.get_enrolled_nodes();
    assert_eq!(nodes.len(), 2);
}

#[test]
fn test_enroll_returns_peers() {
    let manager = create_test_manager();

    manager.register_enrolled_node(
        "node-1".to_string(),
        "host1".to_string(),
        "127.0.0.1:9000".to_string(),
        vec![],
    );

    let token = manager.generate_token(24, vec![]);
    let req = EnrollmentRequest {
        token: token.token,
        node_id: "node-2".to_string(),
        hostname: "host2".to_string(),
        hostnames: vec![],
        ips: vec![],
        tags: vec![],
    };

    let resp = manager.enroll(req).unwrap();

    assert_eq!(resp.peers.len(), 1);
    assert_eq!(resp.peers[0].node_id, "node-1");
    assert_eq!(resp.peers[0].addr, "127.0.0.1:9000");
}

#[test]
fn test_sign_node_cert() {
    let manager = create_test_manager();
    let cert = manager
        .sign_node_cert("node-1", vec!["localhost".to_string()], vec![])
        .unwrap();

    assert!(!cert.cert_pem.is_empty());
    assert!(!cert.key_pem.is_empty());
    assert_eq!(cert.node_id, "node-1");
}
