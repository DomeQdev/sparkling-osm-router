import { createProfile, getNearestNodes, getRoute, searchNodes, searchWays } from "../RustModules";
import { Location } from "../typings";
import Graph from "./Graph";
import RouteQueue from "./RouteQueue";

type HighwayValue =
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

type RailwayValue =
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

export type ProfileOptions = (
    | {
          key: "highway";
          penalties: [HighwayValue | HighwayValue[], number][];
      }
    | {
          key: "railway";
          penalties: [RailwayValue | RailwayValue[], number][];
      }
) & {
    accessTags?: string[];
    onewayTags?: string[];
    exceptTags?: string[];
};

class Profile {
    profileId: number;

    constructor(profile: ProfileOptions) {
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

        this.profileId = createProfile(
            JSON.stringify({
                key: profile.key,
                penalties: convertedPenalties,
                access_tags: Array.from(new Set([...(profile.accessTags ?? []), "access"])),
                oneway_tags: Array.from(new Set([...(profile.onewayTags ?? []), "oneway"])),
                except_tags: profile.exceptTags ?? [],
            })
        );
    }

    get graph(): Graph {
        return undefined as any; // This will be set by the Graph class when creating a Profile instance.
    }

    getNearestNodes = ([lon, lat]: Location, limit: number) => {
        if (this.graph.graphId === null) throw new Error("Graph is not loaded.");

        return getNearestNodes(this.graph.graphId, this.profileId, lon, lat, limit);
    };

    searchNodes = ([lon, lat]: Location, radius: number) => {
        if (this.graph.graphId === null) throw new Error("Graph is not loaded.");

        return searchNodes(this.graph.graphId, this.profileId, lon, lat, radius);
    };

    searchWays = ([lon, lat]: Location, radius: number) => {
        if (this.graph.graphId === null) throw new Error("Graph is not loaded.");

        return searchWays(this.graph.graphId, this.profileId, lon, lat, radius);
    };

    getRoute = async (startNode: number, endNode: number) => {
        if (this.graph.graphId === null) throw new Error("Graph is not loaded.");

        return getRoute(this.graph.graphId, this.profileId, startNode, endNode);
    };

    createRouteQueue = (enableProgressBar?: boolean, maxConcurrency?: number) => {
        if (this.graph.graphId === null) throw new Error("Graph is not loaded.");

        return new RouteQueue(this.graph.graphId, this.profileId, enableProgressBar, maxConcurrency);
    };
}

export default Profile;
