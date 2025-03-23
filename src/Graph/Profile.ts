export type HighwayValue =
    | "motorway"
    | "motorway_link"
    | "trunk"
    | "trunk_link"
    | "primary"
    | "primary_link"
    | "secondary"
    | "secondary_link"
    | "tertiary"
    | "tertiary_link"
    | "unclassified"
    | "residential"
    | "service"
    | "living_street"
    | "pedestrian"
    | "track"
    | "path"
    | "footway"
    | "cycleway"
    | "bridleway"
    | "steps"
    | "corridor"
    | "elevator"
    | "default";

export type RailwayValue =
    | "rail"
    | "light_rail"
    | "subway"
    | "tram"
    | "monorail"
    | "narrow_gauge"
    | "funicular"
    | "preserved"
    | "miniature"
    | "default";

export type KeyToValueType = {
    highway: HighwayValue;
    railway: RailwayValue;
};

export type VehicleType =
    | "foot"
    | "bicycle"
    | "motorcar"
    | "motorcycle"
    | "psv"
    | "train"
    | "subway"
    | "tram";

/**
 * Profile configuration for routing that defines penalties/weights for different way types.
 * The type of values allowed in penalties depends on the selected key.
 */
export type Profile =
    | {
          /**
           * OSM "highway" key used for routing road networks
           */
          key: "highway";

          /**
           * List of penalties/weights for different highway values.
           * Related highway types can be grouped for the same penalty.
           */
          penalties: [HighwayValue | HighwayValue[], number][];

          /**
           * Type of vehicle used for routing, affects access restrictions and turn restrictions
           */
          vehicleType?: VehicleType;
      }
    | {
          /**
           * OSM "railway" key used for routing railway networks
           */
          key: "railway";

          /**
           * List of penalties/weights for different railway values.
           * Related railway types can be grouped for the same penalty.
           */
          penalties: [RailwayValue | RailwayValue[], number][];

          /**
           * Type of vehicle used for routing, affects access restrictions and turn restrictions
           */
          vehicleType?: VehicleType;
      };

/**
 * Converts the new profile format to the legacy format used internally by the routing engine.
 * @param profile Profile configuration in the new format
 * @returns Profile in the legacy format that can be passed to the routing engine
 */
export const convertProfileFormat = (profile: Profile): string => {
    const convertedPenalties: Record<string, number> = {};

    for (const [key, value] of profile.penalties) {
        if (Array.isArray(key)) {
            for (const k of key) {
                convertedPenalties[k] = value;
            }
        } else {
            convertedPenalties[key] = value;
        }
    }

    return JSON.stringify({
        key: profile.key,
        penalties: convertedPenalties,
        vehicle_type: profile.vehicleType,
    });
};
