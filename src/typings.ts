export type Location = [lon: number, lat: number];

export interface RouteResult {
    nodes: number[];
}

export interface OsmNode {
    id: number;
    location: Location;
}

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

export type ProfileConfig = (
    | {
          key: "highway";
          penalties: [HighwayValue | HighwayValue[], number][];
      }
    | {
          key: "railway";
          penalties: [RailwayValue | RailwayValue[], number][];
      }
) & {
    id: string;
    accessTags?: string[];
    onewayTags?: string[];
    exceptTags?: string[];
    disallowMotorroad?: boolean;
    disableRestrictions?: boolean;
};
