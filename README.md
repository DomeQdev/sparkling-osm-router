# ✨ Sparkling-OSM-Router ✨

A blazingly fast, highly-customizable, and memory-efficient routing engine for OpenStreetMap data, built with Rust 🦀 and TypeScript 🔷. Designed for performance-critical applications like generating GTFS `shapes.txt`, batch routing, and geospatial analysis.

## 🚀 Key Features

-   **High-Performance Rust Core**: All heavy lifting (graph processing, routing) is done in native Rust code via [Neon](https://neon-bindings.com/) for maximum speed.
-   **Customizable Routing Profiles**: Define your own routing rules! Create profiles for cars, trains, pedestrians, or anything you can imagine based on OSM tags (`highway`, `railway`, etc.) and assign custom penalties.
-   **Advanced A\* Routing**: Utilizes a highly optimized A\* algorithm with precomputed backward costs (ALT heuristic) for incredibly fast routing, especially across multiple waypoints.
-   **Intelligent Binary Caching**: Downloads and parses OSM data from the Overpass API once, then stores a hyper-optimized binary representation of the graph for near-instantaneous loads on subsequent runs.
-   **Asynchronous Batch Processing**: A built-in `RouteQueue` leverages a parallel thread pool to process thousands of routes concurrently, with progress tracking.
-   **Geospatial Queries**: Quickly find all nodes or ways within a given radius from a specific point.
-   **Helper Utilities**: Comes with tools to `simplifyShape` (Ramer-Douglas-Peucker) and `offsetShape` for post-processing route geometries.

## 📦 Installation

```bash
npm install sparkling-osm-router
```

The package will build the Rust core during the `postinstall` step.

## 🏁 Quick Start

Here's a complete example of loading a graph for a specific area, defining a car routing profile, finding the nearest nodes to your start/end points, and calculating a route.

```typescript
import { Graph, Location } from "sparkling-osm-router";

const FRANKFURT_AIRPORT_AREA: Location[] = [
    [8.529, 50.062],
    [8.59, 50.062],
    [8.59, 50.02],
    [8.529, 50.02],
];

const graph = new Graph({
    filePath: "./cache/fra-airport.bin",
    ttlDays: 7, // Re-download data from Overpass if cache is older than 7 days
    overpassGraph: {
        bounds: FRANKFURT_AIRPORT_AREA,
        query: ["way[highway]"], // Fetch all ways with a 'highway' tag
    },
});

const carProfile = new graph.Profile({
    id: "car",
    key: "highway", // The OSM tag to base penalties on
    penalties: [
        [["motorway", "motorway_link"], 1], // Lowest penalty = fastest
        [["trunk", "trunk_link"], 1.1],
        [["primary", "primary_link"], 1.2],
        [["secondary", "secondary_link"], 1.5],
        [["tertiary", "tertiary_link"], 1.8],
        ["residential", 2.5],
        ["service", 5],
        ["default", 30], // Penalty for any 'highway' tag not listed above (not recommended)
    ],
});

await graph.loadGraph();

const startPoint: Location = [8.5705, 50.0522]; // Frankfurt Airport Terminal 1
const endPoint: Location = [8.5401, 50.0471]; // The Squaire

const startNodeId = carProfile.getNearestNode(startPoint);
const endNodeId = carProfile.getNearestNode(endPoint);

if (!startNodeId || !endNodeId) {
    throw new Error("Could not find nearest nodes for start or end point.");
}

const route = await carProfile.getRoute([startNodeId, endNodeId]);
console.log("Route found:", carProfile.getShape(route));

graph.unloadGraph();
```

## 📚 API Reference

### `Graph`

#### `new Graph(options: GraphOptions)`

Initializes a new graph instance.

-   `options.filePath`: `string` - Path to the binary cache file. The directory will be created if it doesn't exist.
-   `options.ttlDays`: `number` - Time-to-live for the cache file. If the file is older than this, it will be rebuilt.
-   `options.overpassGraph.bounds`: `Location[]` - A polygon defining the geographical area to query.
-   `options.overpassGraph.query`: `string[]` - An array of Overpass query parts (e.g., `way[highway]`, `way[railway]`).
-   `options.overpassGraph.ignoreTurnRestrictions?`: `boolean` - Set to `true` to disable turn restriction processing. Defaults to `false`.

#### `graph.Profile`

#### `graph.loadGraph(): Promise<number>`

Loads the graph from the binary cache or builds it from the Overpass API if needed. Returns the numerical `graphId`.

#### `graph.unloadGraph(): boolean`

Removes the graph and all associated route queues from memory. Returns `true` if successful. It's crucial to call this when you're done to free up resources.

---

### `Profile`

Represents a set of rules for routing on a `Graph`.

#### `new graph.Profile(options: ProfileOptions)`

Creates a new profile associated with the parent graph.

-   `options.id`: `string` - A unique ID for this profile (e.g., "car", "pedestrian").
-   `options.key`: `"highway" | "railway"` - The primary OSM tag key to use for penalties.
-   `options.penalties`: `[string | string[], number][]` - An array defining the cost for traversing ways with specific tag values. Lower penalty is better. A `default` key can be used as a fallback.
-   `options.accessTags?`: `string[]` - Additional tags to check for access rights (e.g., `motorcar`).
-   `options.onewayTags?`: `string[]` - Additional tags to check for oneway streets.
-   `options.exceptTags?`: `string[]` - Vehicle types to consider for `except` tags on turn restrictions.

#### `profile.getRoute(waypoints: number[]): Promise<RouteResult | null>`

Calculates the optimal route through a series of OSM node IDs.

#### `profile.getNearestNode(location: Location): number | null`

Finds the closest routable node in the graph to the given `[lon, lat]` coordinates.

#### `profile.getNode(nodeId: number): OsmNode | null`

Retrieves the full data for a single OSM node, including its ID, location, and tags.

#### `profile.getShape(route: RouteResult): Location[]`

Converts a `RouteResult` object into an array of `[lon, lat]` coordinates, forming the route's geometry.

#### `profile.getNodesInRadius(center: Location, radiusMeters: number): OsmNode[]`

Finds all OSM nodes within a specified radius.

#### `profile.getWaysInRadius(center: Location, radiusMeters: number): OsmWay[]`

Finds all OSM ways that fall at least partially within a specified radius.

#### `profile.createRouteQueue(enableProgressBar?: boolean, maxConcurrency?: number): RouteQueue`

Creates a dedicated queue for high-throughput batch routing using this profile.

---

### `RouteQueue`

Manages batch processing of many route requests in parallel.

#### `queue.enqueueRoute(routeId: string, waypoints: number[]): string`

Adds a new routing task to the queue. `routeId` is a custom identifier you provide to track the result.

#### `queue.awaitAll(callback): Promise<void>`

Starts processing the queue. This is the main execution method.
The `callback` function `(id: string, result: RouteResult | null, error?: Error) => void` is called for each completed route.

```typescript
// Example of using the RouteQueue
const queue = carProfile.createRouteQueue(true); // true enables a CLI progress bar

queue.enqueueRoute("route-1", [123, 456]);
queue.enqueueRoute("route-2", [789, 101]);

const results = new Map();
await queue.awaitAll((id, result, error) => {
    if (error) {
        console.error(`Error processing ${id}:`, error);
    } else {
        console.log(`Completed ${id}`);
        results.set(id, result);
    }
});

console.log("All routes processed!");
```

#### `queue.getStatus(): QueueStatus`

Returns the current status of the queue (`{ queuedTasks, activeTasks, isEmpty }`).

#### `queue.clear(): boolean`

Clears the queue. Can only be called when not processing.

## 🔧 Tools

#### `simplifyShape(shape: Location[], epsilon: number): Location[]`

Simplifies a route's geometry using the Ramer-Douglas-Peucker algorithm to reduce the number of points while preserving the general shape.

#### `offsetShape(shape: Location[], offsetMeters: number): Location[]`

Creates a new shape that is a parallel offset of the original, useful for visualizing distinct directions on a two-way road.
