Feature: thermo-core

  The features uorust admitted for the thermo-core package, each with the
  interface calls that realize it and the acceptance rows that accept it.

  @FT-01 @build
  Scenario: thermo_core: Thermostat control: decides from a reading and a target whether to heat.
    Given the record Interface::featureThermoCore
    When the interface call Interface::callThermoCoreShouldHeat is made
    And the interface call Interface::callThermoCoreWithinBand is made
    And the interface call Interface::callThermoCoreNextMode is made
    And the interface call Interface::callThermoCoreHalt is made
    And the interface call Interface::callThermoCoreMidpoint is made
    Then the acceptance row Interface::acDocThermoCoreShouldHeat holds
    And the acceptance row Interface::acThermoCoreThermoCoreBandIsInclusive holds
    And the acceptance row Interface::acThermoCoreThermoCoreHeatsWhenCold holds
    And the acceptance row Interface::acThermoCoreThermoCoreIdleRoomThatIsColdHeats holds
    And the acceptance row Interface::acThermoCoreThermoCoreMidpointOfABand holds

  @FT-02 @build
  Scenario: thermo_core::band: Temperature bands, counters and readings.
    Given the record Interface::featureThermoCoreBand
    When the interface call Interface::callThermoCoreBandWidth is made
    And the interface call Interface::callThermoCoreBandCheckedMidpoint is made
    And the interface call Interface::callThermoCoreBandTick is made
    And the interface call Interface::callThermoCoreBandShortfall is made
    And the interface call Interface::callThermoCoreBandFreezing is made
    And the interface call Interface::callThermoCoreBandDelta is made
    And the interface call Interface::callThermoCoreBandChecked is made
    And the interface call Interface::callThermoCoreBandIsCelsius is made
    And the interface call Interface::callThermoCoreBandAbove is made
    And the interface call Interface::callThermoCoreBandFirstReading is made
    And the interface call Interface::callThermoCoreBandTargetOr is made
    And the interface call Interface::callThermoCoreBandHeatFor is made
    And the interface call Interface::callThermoCoreBandFanLabel is made
    And the interface call Interface::callThermoCoreBandLabelled is made
    Then the acceptance row Interface::acThermoCoreThermoCoreBandBandsMeasureAndCount holds
    And the acceptance row Interface::acThermoCoreThermoCoreBandCountersAndSignedReadings holds

  @FT-03 @build
  Scenario: thermo_core::config: Reads the target temperature from a file or the environment.
    Given the record Interface::featureThermoCoreConfig
    When the interface call Interface::callThermoCoreConfigParseTarget is made
    And the interface call Interface::callThermoCoreConfigLoadTarget is made
    And the interface call Interface::callThermoCoreConfigTargetFromEnv is made
    Then the acceptance row Interface::acThermoCoreConfigParsesAWholeNumber holds

  @FT-04 @build
  Scenario: thermo_core::sensor: Temperature sensors the controller reads from.
    Given the record Interface::featureThermoCoreSensor
    When the interface call Interface::callThermoCoreSensorSensorRead is made
    And the interface call Interface::callThermoCoreSensorSensorCalibrated is made
    Then the acceptance row Interface::acThermoCoreThermoCoreSensorAFixedSensorReadsItsValue holds
