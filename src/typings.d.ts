export type Location = [number, number];

/**
 * Configuration for a routing profile.
 */
export type Profile = {
    /**
     * The OSM tag key to consider for routing (e.g., "highway").
     * See https://wiki.openstreetmap.org/wiki/Tags#Keys_and_values
     */
    key: string;

    /**
     * Map of penalties for different OSM tag values (e.g., {"motorway": 1, "residential": 3}),
     * including a default value for tags not explicitly specified.
     * Don't include the default value if you don't want routing over not explicitly specified tags.
     */
    penalties: Record<string | "default", number>;
};

/**
 * Configuration options for the routing graph.
 */
export type GraphOptions = {
    /**
     * Options related to fetching data from OpenStreetMap.
     */
    osmGraph: OSMGraphOptions;

    /**
     * Profile used for route calculations.
     */
    profile: Profile;
};

/**
 * Configuration options for fetching OpenStreetMap data.
 */
export type OSMGraphOptions = {
    /**
     * Path to the OSM data file.
     */
    path: string;

    /**
     * Time to live for cached data in hours.
     */
    ttl: number;

    /**
     * Geographic boundaries of the query area.
     */
    bounds: Location[];

    /**
     * Query for the Overpass API.
     */
    overpassQuery: string;
};

/**
 * Result of a routing process.
 */
export type RouteResult = {
    /**
     * List of node IDs that form the route.
     */
    nodes: number[];

    /**
     * List of way IDs that form the route.
     */
    ways: number[];
};
