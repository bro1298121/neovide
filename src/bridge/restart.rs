use rmpv::Value;

#[derive(Clone, Debug, PartialEq)]
pub struct RestartDetails {
    pub listen_addr: String,
    /// Channel-specific properties from the old server (e.g. `detach`),
    /// which should be restored on the new server via `nvim_chan_set(0, ...)`.
    pub chan_config: Option<Value>,
}

impl RestartDetails {
    pub fn from_values(arguments: &[Value]) -> Option<Self> {
        match arguments {
            [listen_addr] => Some(Self {
                listen_addr: listen_addr.as_str()?.to_string(),
                chan_config: None,
            }),
            [listen_addr, chan_config @ Value::Map(_)] => Some(Self {
                listen_addr: listen_addr.as_str()?.to_string(),
                chan_config: Some(chan_config.clone()),
            }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RestartDetails;
    use rmpv::Value;

    #[test]
    fn parses_listen_addr_restart_payload() {
        let details = RestartDetails::from_values(&[Value::from("/tmp/nvim.sock")]);

        assert_eq!(
            details,
            Some(RestartDetails { listen_addr: "/tmp/nvim.sock".to_string(), chan_config: None })
        );
    }

    #[test]
    fn parses_restart_payload_with_chan_config() {
        let chan_config = Value::Map(vec![(Value::from("detach"), Value::from(false))]);
        let details = RestartDetails::from_values(&[Value::from("/tmp/nvim.sock"), chan_config.clone()]);

        assert_eq!(
            details,
            Some(RestartDetails {
                listen_addr: "/tmp/nvim.sock".to_string(),
                chan_config: Some(chan_config)
            })
        );
    }

    #[test]
    fn rejects_restart_payload_with_extra_arguments() {
        let details =
            RestartDetails::from_values(&[Value::from("/tmp/nvim.sock"), Value::from("echo 1")]);

        assert_eq!(details, None);
    }

    #[test]
    fn rejects_restart_payload_with_invalid_type() {
        let details = RestartDetails::from_values(&[Value::from("/tmp/nvim.sock"), Value::from(1)]);

        assert_eq!(details, None);
    }
}
