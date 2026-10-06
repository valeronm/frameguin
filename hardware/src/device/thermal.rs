//! The EC's temperature sensors and fans, read and never set.

use std::sync::Arc;

use frameguin_contract::{
    DeviceError, DeviceResult, Fan, FanDuty, Sensor, SensorReading, ThermalControl, ThermalFeature,
    ThermalLayout, ThermalState, Thresholds,
};

use crate::ec::{Ec, ThermalEc};
use crate::thermal;

pub struct Thermal {
    ec: Arc<dyn ThermalEc>,
    layout: ThermalLayout,
    features: Vec<ThermalFeature>,
}

impl Thermal {
    pub(crate) fn detect(ec: &Arc<Ec>) -> Option<Self> {
        Self::new(ec.clone())
    }

    /// None where the memmap does not answer or reports no sensor and no
    /// fan. The thresholds and the fan duty are each probed by their getter's
    /// own read.
    pub fn new(ec: Arc<dyn ThermalEc>) -> Option<Self> {
        let (bytes, words) = ec.thermal_memmap().ok()?;
        let sensors: Vec<Sensor> = (0u8..)
            .zip(&bytes)
            .filter(|(_, byte)| thermal::temperature(**byte).is_some())
            .map(|(index, _)| Sensor {
                index,
                name: ec.sensor_name(index).ok().filter(|name| !name.is_empty()),
            })
            .collect();
        let fans: Vec<u8> = (0u8..)
            .zip(&words)
            .filter(|(_, word)| thermal::fan_rpm(**word).is_some())
            .map(|(index, _)| index)
            .collect();
        if sensors.is_empty() && fans.is_empty() {
            return None;
        }
        let mut features = Vec::new();
        if !sensors.is_empty()
            && sensors
                .iter()
                .all(|sensor| ec.thresholds(sensor.index).is_ok())
        {
            features.push(ThermalFeature::Thresholds);
        }
        if !fans.is_empty() && fans.iter().all(|index| ec.fan_duty(*index).is_ok()) {
            features.push(ThermalFeature::FanDuty);
        }
        Some(Self {
            ec,
            layout: ThermalLayout { sensors, fans },
            features,
        })
    }
}

impl ThermalControl for Thermal {
    async fn features(&self) -> DeviceResult<Vec<ThermalFeature>> {
        Ok(self.features.clone())
    }

    async fn layout(&self) -> DeviceResult<ThermalLayout> {
        Ok(self.layout.clone())
    }

    /// A slot present at detection that reads as absent now is left out.
    async fn state(&self) -> DeviceResult<ThermalState> {
        let (bytes, words) = self.ec.thermal_memmap()?;
        let sensors = self
            .layout
            .sensors
            .iter()
            .filter_map(|sensor| {
                let byte = *bytes.get(usize::from(sensor.index))?;
                Some(SensorReading {
                    index: sensor.index,
                    temperature: thermal::temperature(byte)?,
                })
            })
            .collect();
        let fans = self
            .layout
            .fans
            .iter()
            .filter_map(|index| {
                let word = *words.get(usize::from(*index))?;
                Some(Fan {
                    index: *index,
                    rpm: thermal::fan_rpm(word)?,
                })
            })
            .collect();
        Ok(ThermalState { sensors, fans })
    }

    async fn thresholds(&self) -> DeviceResult<Vec<Thresholds>> {
        if !self.features.contains(&ThermalFeature::Thresholds) {
            return Err(DeviceError::NotSupported(
                "the EC does not report thermal thresholds".into(),
            ));
        }
        self.layout
            .sensors
            .iter()
            .map(|sensor| {
                self.ec
                    .thresholds(sensor.index)
                    .map(|raw| thermal::thresholds(sensor.index, raw))
            })
            .collect()
    }

    async fn fan_duties(&self) -> DeviceResult<Vec<FanDuty>> {
        if !self.features.contains(&ThermalFeature::FanDuty) {
            return Err(DeviceError::NotSupported(
                "the EC does not report fan duty".into(),
            ));
        }
        self.layout
            .fans
            .iter()
            .map(|&index| {
                let percent = self.ec.fan_duty(index)?;
                Ok(FanDuty { index, percent })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use frameguin_contract::{
        Fan, FanDuty, Sensor, SensorReading, Temperature, ThermalControl, ThermalFeature,
        ThermalLayout,
    };

    use super::Thermal;
    use crate::testing::{Vents, ready};

    #[test]
    fn detection_keeps_the_present_sensors_and_fans_with_their_names() {
        let thermal = Thermal::new(Arc::new(Vents::default())).expect("the memmap answered");
        assert_eq!(
            ready(thermal.layout()),
            Ok(ThermalLayout {
                sensors: vec![
                    Sensor {
                        index: 0,
                        name: Some("sensor0".into())
                    },
                    Sensor {
                        index: 2,
                        name: Some("sensor2".into())
                    },
                ],
                fans: vec![0],
            })
        );
        assert_eq!(
            ready(thermal.features()),
            Ok(vec![ThermalFeature::Thresholds, ThermalFeature::FanDuty])
        );
    }

    #[test]
    fn a_reading_carries_each_present_sensor_and_fan() {
        let thermal = Thermal::new(Arc::new(Vents::default())).unwrap();
        let state = ready(thermal.state()).unwrap();
        assert_eq!(
            state.sensors,
            [
                SensorReading {
                    index: 0,
                    temperature: Temperature::Kelvin(340)
                },
                SensorReading {
                    index: 2,
                    temperature: Temperature::Unpowered
                },
            ]
        );
        assert_eq!(
            state.fans,
            [Fan {
                index: 0,
                rpm: 2400
            }]
        );
    }

    #[test]
    fn thresholds_are_read_for_every_sensor() {
        let thermal = Thermal::new(Arc::new(Vents::default())).unwrap();
        let thresholds = ready(thermal.thresholds()).unwrap();
        assert_eq!(
            thresholds.iter().map(|t| t.index).collect::<Vec<_>>(),
            [0, 2]
        );
        assert_eq!(thresholds[0].high, Some(361));
    }

    #[test]
    fn firmware_without_the_threshold_read_offers_none() {
        let vents = Vents {
            thresholds: None,
            ..Vents::default()
        };
        let thermal = Thermal::new(Arc::new(vents)).unwrap();
        assert_eq!(ready(thermal.features()), Ok(vec![ThermalFeature::FanDuty]));
        assert!(ready(thermal.thresholds()).is_err());
    }

    #[test]
    fn a_duty_is_read_for_every_fan() {
        let thermal = Thermal::new(Arc::new(Vents::default())).unwrap();
        assert_eq!(
            ready(thermal.fan_duties()),
            Ok(vec![FanDuty {
                index: 0,
                percent: 45
            }])
        );
    }

    #[test]
    fn firmware_without_the_fan_duty_read_offers_none() {
        let vents = Vents {
            fan_duty: None,
            ..Vents::default()
        };
        let thermal = Thermal::new(Arc::new(vents)).unwrap();
        assert_eq!(
            ready(thermal.features()),
            Ok(vec![ThermalFeature::Thresholds])
        );
        assert!(ready(thermal.fan_duties()).is_err());
    }

    #[test]
    fn a_memmap_that_does_not_answer_is_no_device() {
        let vents = Vents {
            refusing: true,
            ..Vents::default()
        };
        assert!(Thermal::new(Arc::new(vents)).is_none());
    }

    #[test]
    fn a_memmap_reporting_nothing_present_is_no_device() {
        let vents = Vents {
            sensors: vec![0xff; 16],
            fans: vec![0xffff; 4],
            ..Vents::default()
        };
        assert!(Thermal::new(Arc::new(vents)).is_none());
    }
}
