# ✨ Sparkling OSM Router ✨

Sparkling OSM Router is a routing library using OpenStreetMap data. Thanks to implementation in Rust with an interface for JavaScript/TypeScript, it provides efficient route calculation for various transport profiles.

## Installation

```bash
npm install sparkling-osm-router
```

## Basic Usage

```typescript
import { Graph, Profile } from "sparkling-osm-router";

// Define a car routing profile
const carProfile: Profile = {
    key: "highway",
    penalties: {
        motorway: 1,
        trunk: 1.1,
        primary: 1.3,
        secondary: 1.5,
        tertiary: 1.7,
        residential: 2.0,
        default: 3.0,
    },
    vehicle_type: "motorcar",
};

// Create graph with OSM data configuration
const graph = new Graph({
    osmGraph: {
        path: "./map-data/berlin.osm.pbf",
        ttl: 24, // Cache time in hours
        bounds: [
            [13.3, 52.4],
            [13.5, 52.6],
        ], // Berlin area
        overpassQuery: `way["highway"~"^(motorway|motorway_link|primary|primary_link|secondary|secondary_link|tertiary|tertiary_link|unclassified|residential|service)$"]`,
    },
    profile: carProfile,
});

// Load the graph
await graph.loadGraph();

// Find nodes near specific coordinates
const startNodeId = graph.getNearestNode([13.385, 52.516]); // Brandenburg Gate
const endNodeId = graph.getNearestNode([13.428, 52.523]); // Alexanderplatz

// Calculate route
const route = await graph.getRoute(startNodeId, endNodeId);
console.log(`Route found with ${route.nodes.length} nodes via ${route.ways.length} ways`);

// Get the route shape as coordinates
const shape = graph.getShape({ nodes: route.nodes });

// Cleanup when done
graph.cleanup();
```

## Advanced Features

### Route Queues

For batch processing multiple routes:

```typescript
import { Graph } from "sparkling-osm-router";

// Create graph and load data...
await graph.loadGraph();

// Create a route queue with progress bar
const queue = graph.createRouteQueue(true);

// Add routes to queue
queue.enqueueRoute("route1", { startNode: 123, endNode: 456 });
queue.enqueueRoute("route2", { startNode: 789, endNode: 101112 });
queue.enqueueRoute("route3", { startNode: 131415, endNode: 161718, bearing: 90 });

// Process all routes
await queue.awaitAll((id, result, error) => {
    if (error) {
        console.error(`Error calculating route ${id}:`, error);
        return;
    }
    console.log(`Route ${id} processed with ${result?.nodes.length || 0} nodes`);
});

// Cleanup
queue.cleanup();
graph.cleanup();
```

### Shape Simplification and Offsetting

```typescript
// Get route shape
const shape = graph.getShape({ nodes: route.nodes });

// Simplify the shape (for better performance when rendering)
const simplifiedShape = graph.getSimplifiedShape({ nodes: route.nodes }, 0.00001);

// Create offset shape (useful for drawing parallel paths)
const rightOffset = graph.offsetShape(shape, 5, 1); // 5 meters to the right
const leftOffset = graph.offsetShape(shape, 5, -1); // 5 meters to the left
```

## Routing Profiles

Routing profiles allow you to customize how routes are calculated:

```typescript
// Walking profile example
const walkingProfile: Profile = {
    key: "highway",
    penalties: {
        footway: 1,
        path: 1,
        pedestrian: 1,
        steps: 1.5,
        residential: 2,
        living_street: 1.5,
        default: 3.0,
    },
    vehicle_type: "foot",
};

// Cycling profile example
const cyclingProfile: Profile = {
    key: "highway",
    penalties: {
        cycleway: 1,
        path: 1.2,
        residential: 1.5,
        tertiary: 2,
        primary: 3,
        default: 4.0,
    },
    vehicle_type: "bicycle",
};
```

## API Reference

### Graph Class

The main class for working with OSM data and routing.

#### Methods:

-   `loadGraph()`: Loads OSM data into memory
-   `getRoute(startNode, endNode, bearing?)`: Calculates route between two nodes
-   `getNearestNode(location)`: Finds nearest node to given coordinates
-   `getNearestNodes(location, limit?, distanceThresholdMultiplier?)`: Finds multiple nearest nodes
-   `searchNearestNode(location, searchString)`: Searches for nodes matching text criteria
-   `getNodes({ nodes })`: Gets data for specified nodes
-   `getWays({ ways })`: Gets data for specified ways
-   `getShape({ nodes })`: Gets shape coordinates for a list of nodes
-   `getSimplifiedShape({ nodes }, epsilon?)`: Gets simplified shape for route
-   `offsetShape(shape, offsetMeters?, offsetSide?)`: Creates offset parallel path
-   `createRouteQueue(enableProgressBar?, maxConcurrency?)`: Creates queue for batch processing
-   `cleanup()`: Cleans up resources

### RouteQueue Class

For efficient batch processing of multiple routes.

#### Methods:

-   `enqueueRoute(id, options)`: Adds route to processing queue
-   `getStatus()`: Gets current queue status
-   `clear()`: Clears queued routes
-   `awaitAll(callback)`: Processes all queued routes
-   `cleanup()`: Cleans up resources

## Performance Tips

-   Use `getSimplifiedShape()` for rendering large routes
-   Use route queues for batch processing
-   Limit your OSM data area to only what's needed
-   Set appropriate TTL values for caching OSM data

## License

MIT
