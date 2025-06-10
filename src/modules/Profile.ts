import { createProfile, getNearestNodes, getRoute } from "../RustModules";
import { Location } from "../typings";
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

    get graphId(): number | null {
        return null;
    }

    getNearestNodes = ([lon, lat]: Location, limit: number, maxDistanceThreshold: number) => {
        if (this.graphId === null) throw new Error("Graph is not loaded.");

        return getNearestNodes(this.graphId, this.profileId, lon, lat, limit, maxDistanceThreshold);
    };

    getRoute = async (startNode: number, endNode: number) => {
        if (this.graphId === null) throw new Error("Graph is not loaded.");

        return getRoute(this.graphId, this.profileId, startNode, endNode);
    };

    createRouteQueue = (enableProgressBar?: boolean, maxConcurrency?: number) => {
        if (this.graphId === null) throw new Error("Graph is not loaded.");

        return new RouteQueue(this.graphId, this.profileId, enableProgressBar, maxConcurrency);
    };
}

export default Profile;
