use flockmind::auth::certs::*;
use tempfile::TempDir;

#[test]
fn test_ca_certificate_generate() {
    let ca = CaCertificate::generate("test-cluster").unwrap();
    assert!(!ca.cert_pem.is_empty());
    assert!(ca.cert_pem.contains("BEGIN CERTIFICATE"));
}

#[test]
fn test_ca_certificate_save_load() {
    let temp_dir = TempDir::new().unwrap();
    let cert_path = temp_dir.path().join("ca.crt");
    let key_path = temp_dir.path().join("ca.key");

    let ca = CaCertificate::generate("test-cluster").unwrap();
    ca.save(&cert_path, &key_path).unwrap();

    assert!(cert_path.exists());
    assert!(key_path.exists());

    let loaded = CaCertificate::load(&cert_path, &key_path).unwrap();
    assert_eq!(ca.cert_pem, loaded.cert_pem);
}

#[test]
fn test_ca_sign_node() {
    let ca = CaCertificate::generate("test-cluster").unwrap();
    let node_cert = ca
        .sign_node(
            "node-1",
            vec!["localhost".to_string()],
            vec!["127.0.0.1".to_string()],
        )
        .unwrap();

    assert!(!node_cert.cert_pem.is_empty());
    assert!(!node_cert.key_pem.is_empty());
    assert_eq!(node_cert.node_id, "node-1");
    assert!(node_cert.cert_pem.contains("BEGIN CERTIFICATE"));
    assert!(node_cert.key_pem.contains("BEGIN PRIVATE KEY"));
}

#[test]
fn test_node_certificate_save_load() {
    let temp_dir = TempDir::new().unwrap();
    let cert_path = temp_dir.path().join("node.crt");
    let key_path = temp_dir.path().join("node.key");

    let ca = CaCertificate::generate("test-cluster").unwrap();
    let node_cert = ca.sign_node("node-1", vec![], vec![]).unwrap();

    node_cert.save(&cert_path, &key_path).unwrap();

    assert!(cert_path.exists());
    assert!(key_path.exists());

    let loaded = NodeCertificate::load(&cert_path, &key_path).unwrap();
    assert_eq!(node_cert.cert_pem, loaded.cert_pem);
    assert_eq!(node_cert.key_pem, loaded.key_pem);
    assert_eq!(loaded.node_id, "node-1");
}

#[test]
fn test_node_certificate_cert_der() {
    let ca = CaCertificate::generate("test-cluster").unwrap();
    let node_cert = ca.sign_node("node-1", vec![], vec![]).unwrap();

    let der = node_cert.cert_der().unwrap();
    assert!(!der.as_ref().is_empty());
}

#[test]
fn test_ca_clone() {
    let ca = CaCertificate::generate("test-cluster").unwrap();
    let cloned = ca.clone();

    assert_eq!(ca.cert_pem, cloned.cert_pem);

    let cert1 = ca.sign_node("node-1", vec![], vec![]).unwrap();
    let cert2 = cloned.sign_node("node-2", vec![], vec![]).unwrap();

    assert!(!cert1.cert_pem.is_empty());
    assert!(!cert2.cert_pem.is_empty());
}

#[test]
fn test_sign_node_with_multiple_hostnames() {
    let ca = CaCertificate::generate("test-cluster").unwrap();
    let node_cert = ca
        .sign_node(
            "node-1",
            vec![
                "localhost".to_string(),
                "node1.local".to_string(),
                "node1.cluster.internal".to_string(),
            ],
            vec!["127.0.0.1".to_string(), "192.168.1.100".to_string()],
        )
        .unwrap();

    assert!(!node_cert.cert_pem.is_empty());
}

#[test]
fn test_create_tls_config() {
    // Install crypto provider for rustls
    let _ = rustls::crypto::ring::default_provider().install_default();
    
    let ca = CaCertificate::generate("test-cluster").unwrap();
    let node_cert = ca
        .sign_node("node-1", vec!["localhost".to_string()], vec![])
        .unwrap();

    let _config = create_tls_config(&node_cert, &ca.cert_pem).unwrap();
    // Config was created successfully
}

#[test]
fn test_create_client_tls_config() {
    // Install crypto provider for rustls
    let _ = rustls::crypto::ring::default_provider().install_default();
    
    let ca = CaCertificate::generate("test-cluster").unwrap();
    let node_cert = ca
        .sign_node("node-1", vec!["localhost".to_string()], vec![])
        .unwrap();

    let _config = create_client_tls_config(&node_cert, &ca.cert_pem).unwrap();
}
