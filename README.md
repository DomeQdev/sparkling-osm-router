# ✨ Sparkling OSM Router ✨

Sparkling OSM Router is a routing library using OpenStreetMap data. Thanks to implementation in Rust with an interface for JavaScript/TypeScript, it provides efficient route calculation for various transport profiles.

## Installation

```bash
npm install sparkling-osm-router
```

## Basic Usage

Below is an example of basic library usage:

```typescript
import Graph from "sparkling-osm-router";
import { join } from "path";

// Configure graph for cars
const carGraph = new Graph({
    osmGraph: {
        path: join(__dirname, "osmGraph.xml"),
        ttl: 24, // cache lifetime in hours
        bounds: [
            [20.54229, 52.257538],
            [21.046434, 52.265235],
        ], // area boundaries (bbox)
        overpassQuery: `way["highway"~"^(motorway|primary|secondary)$"]`, // Overpass API query
    },
    profile: {
        key: "highway", // OSM key for routing
        penalties: {
            default: 300, // if not provided, graph won't route over types not listed in penalties
            motorway: 10, // lowest value = preferred road
            primary: 30,
            secondary: 50,
            residential: 150,
        },
        vehicle_type: "motorcar", // vehicle type for access and turn restrictions
    },
});

// Load the graph
await carGraph.loadGraph();

// Find nearest nodes to the provided coordinates
const startNode = carGraph.getNearestNode([20.924942, 52.272449])[0]; // Get the nearest node
const endNode = carGraph.getNearestNode([21.046434, 52.265235])[0];  // Get the nearest node

// Calculate the route
const route = await carGraph.getRoute(startNode, endNode);

// Get the route shape for visualization
const routeShape = carGraph.getShape(route);

// Clean up resources
carGraph.cleanup();
```

## Configuration

### GraphOptions

Main graph configuration contains the following parameters:

```typescript
{
    osmGraph: {
        path: string;       // Path to OSM data file
        ttl: number;        // Cache lifetime in hours
        bounds: Location[]; // Area boundaries (bbox)
        overpassQuery: string; // Overpass API query
    },
    profile: {
        key: string;        // OSM key, e.g., "highway" or "railway"
        penalties: {        // Weights (penalties) for different values
            default?: number,  // Default value (optional)
            [value: string]: number // Weights for specific values
        },
        vehicle_type?: string // Vehicle type for access and turn restrictions (optional)
                             // Supported values: "foot", "bicycle", "motorcar", "motorcycle",
                             // "psv", "train", "subway", "tram"
    }
}
```

### Routing Profiles

The library allows defining different routing profiles by configuring the OSM key and appropriate weights.

#### Car Profile

```typescript
{
    key: "highway",
    penalties: {
        motorway: 10,
        motorway_link: 10,
        trunk: 10,
        trunk_link: 10,
        primary: 20,
        primary_link: 20,
        secondary: 30,
        secondary_link: 30,
        tertiary: 40,
        tertiary_link: 40,
        unclassified: 50,
        residential: 50,
        service: 100,
        default: 300,
    },
    vehicle_type: "motorcar" // Optional vehicle type for access and turn restrictions
}
```

#### Railway Profile

```typescript
{
    key: "railway",
    penalties: {
        // default is not provided, so the graph won't route over types not listed in penalties, for example, it will ignore subway
        rail: 10,
        light_rail: 20,
    },
    vehicle_type: "train" // Optional vehicle type for access and turn restrictions
}
```

## API

### Graph

#### constructor(options: GraphOptions)

Creates a new graph instance with the provided options.

#### loadGraph(): Promise<void>

Loads graph data. Should be called before using other methods.

#### getNearestNode(location: [lon, lat], limit: number = 1, distanceThresholdMultiplier: number = 5.0): number[]

Returns an array of IDs of the nearest nodes to the provided coordinates.

-   `location` - array [longitude, latitude]
-   `limit` - maximum number of nodes to return (default: 1)
-   `distanceThresholdMultiplier` - multiplier for distance threshold when selecting multiple nodes (default: 5.0)

#### searchNearestNode(location: [lon, lat], searchString: string): { id: number, score: number }

Searches for a node near the given coordinates that best matches the search string in its tags.

-   `location` - array [longitude, latitude]
-   `searchString` - string to search for in node tags (e.g., "100101 Kijowska 01")

Returns a single best match node with its ID and a score indicating the quality of the match.
The function internally searches among nearby nodes (using a larger search radius than getNearestNode)
and evaluates matches in their tag values and keys.

#### getRoute(startNode: number, endNode: number, bearing?: number): Promise<RouteResult>

Calculates the route between two nodes.

-   `startNode` - starting node ID
-   `endNode` - ending node ID
-   `bearing` - optional initial direction in degrees

#### getNodes({ nodes: number[] }): NodeData[]

Returns detailed information about the specified nodes.

-   `nodes` - array of node IDs

#### getWays({ ways: number[] }): WayData[]

Returns detailed information about the specified ways.

-   `ways` - array of way IDs

#### getShape({ nodes: number[] }): Location[]

Returns the route shape as an array of coordinates.

-   `nodes` - array of node IDs from a route result

#### getSimplifiedShape({ nodes: number[] }, epsilon: number = 1e-5): Location[]

Returns a simplified route shape as an array of coordinates using the Ramer-Douglas-Peucker algorithm.

-   `nodes` - array of node IDs from a route result
-   `epsilon` - simplification tolerance (higher = more simplification, default: 1e-5)

#### offsetShape(shape: Location[], offsetMeters: number = 1.5, offsetSide: 1 | -1 = 1): Location[]

Returns an offset shape for given coordinates.

-   `shape` - array of coordinates [longitude, latitude]
-   `offsetMeters` - offset distance in meters (default: 1.5)
-   `offsetSide` - offset side, 1 for right, -1 for left (default: 1)

#### cleanup(): boolean

Releases graph resources.

### RouteQueue

For batch processing of multiple routing tasks with automatic distribution across processor threads.

#### constructor(graph: Graph, maxConcurrency: number = cpus().length - 1)

Creates a new RouteQueue instance.

- `graph` - The graph instance to use for routing
- `maxConcurrency` - Optional maximum number of concurrent tasks

#### enqueueRoute(id: string, options: RouteQueueOptions): string

Adds a route to the processing queue.

- `id` - Unique identifier for this route
- `options` - Route options containing:
  - `startNode` - ID of starting node
  - `endNode` - ID of ending node
  - `bearing` - Optional bearing direction in degrees

#### awaitAll(callback: (id: string, result: RouteResult | null, error?: Error) => void): Promise<void>

Processes all queued route calculations and waits for them to complete.

- `callback` - Function called for each completed route with its ID and result

#### getStatus(): QueueStatus

Gets the current status of the queue.

#### clear(): void

Clears all queued routes that haven't started processing yet.

#### cleanup(): void

Cleans up resources used by the queue.

## Usage Examples

### Car Routing

```typescript
const carGraph = new Graph({
    osmGraph: {
        path: join(__dirname, "osmGraph.xml"),
        ttl: 24,
        bounds: [
            [20.54229, 52.257538],
            [21.046434, 52.265235],
        ],
        overpassQuery: `way["highway"~"^(motorway|motorway_link|trunk|trunk_link|primary|primary_link|secondary|secondary_link|tertiary|tertiary_link|unclassified|minor|residential|living_street|service)$"]`,
    },
    profile: {
        key: "highway",
        penalties: {
            motorway: 10,
            primary: 30,
            secondary: 50,
            tertiary: 70,
            residential: 150,
        },
        vehicle_type: "motorcar",
    },
});

await carGraph.loadGraph();
const start = carGraph.getNearestNode([20.924942, 52.272449])[0];
const end = carGraph.getNearestNode([21.046434, 52.265235])[0];
const route = await carGraph.getRoute(start, end);
const shape = carGraph.getShape(route);
```

### Finding a Public Transport Stop by Name and Coordinates

```typescript
const transportGraph = new Graph({ /* configuration */ });
await transportGraph.loadGraph();

// Find a bus stop by its name and reference number
const busStop = transportGraph.searchNearestNode(
  [21.03324, 52.22679],    // approximate location
  "100101 Kijowska 01"     // search string containing stop ID and name
);

if (busStop) {
  console.log(`Found bus stop with ID: ${busStop.id}, match score: ${busStop.score}`);
  const stopData = transportGraph.getNodes({ nodes: [busStop.id] })[0];
  console.log(`Bus stop tags:`, stopData.tags);
}
```

### Public Transport Routing

```typescript
const railGraph = new Graph({
    osmGraph: {
        path: join(__dirname, "railGraph.xml"),
        ttl: 24,
        bounds: [
            [20.54229, 52.257538],
            [21.046434, 52.265235],
        ],
        overpassQuery: `way["railway"~"^(rail|subway)$"]`,
    },
    profile: {
        key: "railway",
        penalties: {
            rail: 10,
            subway: 20,
        },
        vehicle_type: "train",
    },
});

await railGraph.loadGraph();
// ... further operations as above
```

### Route Shape Simplification

```typescript
// Calculate route
const route = await carGraph.getRoute(startNode, endNode);

// Get full route shape
const fullShape = carGraph.getShape(route);

// Get simplified shape (fewer points, faster rendering)
const simplifiedShape = carGraph.getSimplifiedShape(route, 0.0001);

// Use offset for simplified shape
const offsetShape = carGraph.offsetShape(simplifiedShape, 2.0, 1);
```

### Batch Route Processing with RouteQueue

For efficient batch processing of multiple routes:

```typescript
const graph = new Graph({ /* configuration */ });
await graph.loadGraph();

// Create a route queue
const queue = graph.createRouteQueue();

// Add multiple routes to the queue
queue.enqueueRoute("route1", { 
    startNode: 123456, 
    endNode: 789012
});

queue.enqueueRoute("route2", { 
    startNode: 345678, 
    endNode: 901234,
    bearing: 45
});

// Process all routes and wait for completion
await queue.awaitAll((id, result, error) => {
    if (error) {
        console.error(`Route ${id} failed:`, error);
        return;
    }
    
    if (!result) {
        console.log(`No route found for ${id}`);
        return;
    }
    
    console.log(`Route ${id} found with ${result.nodes.length} points`);
    
    // Process the route result
    const shape = graph.getShape({ nodes: result.nodes });
    saveRoute(id, shape); // Implement this function
});

// Clean up
queue.cleanup();
graph.cleanup();
```

### Parallel Calculation of Multiple Routes

```typescript
const routes = await Promise.all([
    carGraph.getRoute(startNode1, endNode1).then(result => carGraph.getShape(result)),
    carGraph.getRoute(startNode2, endNode2).then(result => carGraph.getShape(result)),
    carGraph.getRoute(startNode3, endNode3).then(result => carGraph.getShape(result)),
]);
```

### Custom Overpass Queries

You can use any Overpass API query to fetch data:

```typescript
const bikeGraph = new Graph({
    osmGraph: {
        path: join(__dirname, "bikeGraph.xml"),
        ttl: 24,
        bounds: [
            [20.54229, 52.257538],
            [21.046434, 52.265235],
        ],
        overpassQuery: `way["highway"~"^(cycleway|path|footway)$"]["bicycle"!="no"]`,
    },
    profile: {
        key: "highway",
        penalties: {
            cycleway: 10,
            path: 20,
            footway: 30,
        },
        vehicle_type: "bicycle",
    },
});
```
