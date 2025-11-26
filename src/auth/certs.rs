use anyhow::{anyhow, Result};
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, IsCa,
    KeyPair, KeyUsagePurpose, SanType,
};
use std::path::Path;
use std::sync::Arc;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};

#[derive(Clone)]
pub struct NodeCertificate {
    pub cert_pem: String,
    pub key_pem: String,
    pub node_id: String,
}

pub struct CaCertificate {
    key_pair: KeyPair,
    cn: String,
    pub cert_pem: String,
}

impl Clone for CaCertificate {
    fn clone(&self) -> Self {
        Self {
            key_pair: KeyPair::from_pem(&self.key_pair.serialize_pem()).unwrap(),
            cn: self.cn.clone(),
            cert_pem: self.cert_pem.clone(),
        }
    }
}

impl CaCertificate {
    fn make_ca_params(cn: &str) -> CertificateParams {
        let mut params = CertificateParams::default();
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::CrlSign,
            KeyUsagePurpose::DigitalSignature,
        ];

        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, cn);
        dn.push(DnType::OrganizationName, "FlockMind");
        params.distinguished_name = dn;
        params
    }

    pub fn generate(cluster_id: &str) -> Result<Self> {
        let cn = format!("FlockMind CA - {}", cluster_id);
        let params = Self::make_ca_params(&cn);

        let key_pair = KeyPair::generate()?;
        let cert = params.self_signed(&key_pair)?;
        let cert_pem = cert.pem();

        Ok(Self {
            key_pair,
            cn,
            cert_pem,
        })
    }

    pub fn load<P: AsRef<Path>>(cert_path: P, key_path: P) -> Result<Self> {
        let cert_pem = std::fs::read_to_string(&cert_path)?;
        let key_pem = std::fs::read_to_string(key_path)?;
        let key_pair = KeyPair::from_pem(&key_pem)?;

        let cn = extract_cn_from_pem(&cert_pem).unwrap_or_else(|_| "FlockMind CA".to_string());

        Ok(Self {
            key_pair,
            cn,
            cert_pem,
        })
    }

    pub fn save<P: AsRef<Path>>(&self, cert_path: P, key_path: P) -> Result<()> {
        std::fs::write(cert_path, &self.cert_pem)?;
        std::fs::write(key_path, self.key_pair.serialize_pem())?;
        Ok(())
    }

    pub fn sign_node(
        &self,
        node_id: &str,
        hostnames: Vec<String>,
        ips: Vec<String>,
    ) -> Result<NodeCertificate> {
        let mut params = CertificateParams::default();
        params.is_ca = IsCa::NoCa;
        params.key_usages = vec![
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::KeyEncipherment,
        ];
        params.extended_key_usages = vec![
            rcgen::ExtendedKeyUsagePurpose::ServerAuth,
            rcgen::ExtendedKeyUsagePurpose::ClientAuth,
        ];

        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, node_id);
        dn.push(DnType::OrganizationName, "FlockMind Node");
        params.distinguished_name = dn;

        let mut sans = vec![SanType::DnsName(node_id.try_into()?)];
        for hostname in hostnames {
            if let Ok(name) = hostname.try_into() {
                sans.push(SanType::DnsName(name));
            }
        }
        for ip in ips {
            if let Ok(addr) = ip.parse() {
                sans.push(SanType::IpAddress(addr));
            }
        }
        params.subject_alt_names = sans;

        let node_key = KeyPair::generate()?;

        let ca_params = Self::make_ca_params(&self.cn);
        let ca_cert = ca_params.self_signed(&self.key_pair)?;
        let cert = params.signed_by(&node_key, &ca_cert, &self.key_pair)?;

        Ok(NodeCertificate {
            cert_pem: cert.pem(),
            key_pem: node_key.serialize_pem(),
            node_id: node_id.to_string(),
        })
    }
}

impl NodeCertificate {
    pub fn load<P: AsRef<Path>>(cert_path: P, key_path: P) -> Result<Self> {
        let cert_pem = std::fs::read_to_string(&cert_path)?;
        let key_pem = std::fs::read_to_string(&key_path)?;

        let node_id = extract_cn_from_pem(&cert_pem)?;

        Ok(Self {
            cert_pem,
            key_pem,
            node_id,
        })
    }

    pub fn save<P: AsRef<Path>>(&self, cert_path: P, key_path: P) -> Result<()> {
        std::fs::write(cert_path, &self.cert_pem)?;
        std::fs::write(key_path, &self.key_pem)?;
        Ok(())
    }

    pub fn cert_der(&self) -> Result<CertificateDer<'static>> {
        let pem = pem::parse(&self.cert_pem)?;
        Ok(CertificateDer::from(pem.contents().to_vec()))
    }

    pub fn key_der(&self) -> Result<PrivateKeyDer<'static>> {
        let pem = pem::parse(&self.key_pem)?;
        Ok(PrivateKeyDer::Pkcs8(pem.contents().to_vec().into()))
    }
}

fn extract_cn_from_pem(pem_str: &str) -> Result<String> {
    let pem = pem::parse(pem_str)?;
    let (_, cert) = x509_parser::parse_x509_certificate(pem.contents())
        .map_err(|e| anyhow!("Failed to parse certificate: {:?}", e))?;

    for attr in cert.subject().iter_common_name() {
        if let Ok(cn) = attr.as_str() {
            return Ok(cn.to_string());
        }
    }

    Err(anyhow!("No CN found in certificate"))
}

pub fn create_tls_config(
    node_cert: &NodeCertificate,
    ca_cert_pem: &str,
) -> Result<Arc<tokio_rustls::rustls::ServerConfig>> {
    use tokio_rustls::rustls::{server::WebPkiClientVerifier, RootCertStore, ServerConfig};

    let cert_chain = vec![node_cert.cert_der()?];
    let key = node_cert.key_der()?;

    let mut root_store = RootCertStore::empty();
    let ca_pem = pem::parse(ca_cert_pem)?;
    let ca_der = CertificateDer::from(ca_pem.contents().to_vec());
    root_store.add(ca_der)?;

    let client_verifier = WebPkiClientVerifier::builder(Arc::new(root_store))
        .build()
        .map_err(|e| anyhow!("Failed to build client verifier: {}", e))?;

    let config = ServerConfig::builder()
        .with_client_cert_verifier(client_verifier)
        .with_single_cert(cert_chain, key)
        .map_err(|e| anyhow!("Failed to build server config: {}", e))?;

    Ok(Arc::new(config))
}

pub fn create_client_tls_config(
    node_cert: &NodeCertificate,
    ca_cert_pem: &str,
) -> Result<Arc<tokio_rustls::rustls::ClientConfig>> {
    use tokio_rustls::rustls::{ClientConfig, RootCertStore};

    let cert_chain = vec![node_cert.cert_der()?];
    let key = node_cert.key_der()?;

    let mut root_store = RootCertStore::empty();
    let ca_pem = pem::parse(ca_cert_pem)?;
    let ca_der = CertificateDer::from(ca_pem.contents().to_vec());
    root_store.add(ca_der)?;

    let config = ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_client_auth_cert(cert_chain, key)
        .map_err(|e| anyhow!("Failed to build client config: {}", e))?;

    Ok(Arc::new(config))
}
