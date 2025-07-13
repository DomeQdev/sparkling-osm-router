import { RouteResult } from "../typings";
import cliProgress from "cli-progress";
import { cpus } from "os";
import {
    clearRouteQueue,
    createRouteQueue,
    enqueueRoute,
    getQueueStatus,
    processQueue,
} from "../RustModules";

class RouteQueue {
    public queueId: number;
    private enableProgressBar: boolean;
    private processing: boolean = false;

    constructor(
        graphId: number,
        profileId: string,
        enableProgressBar: boolean = false,
        maxConcurrency: number = cpus().length - 1
    ) {
        this.queueId = createRouteQueue(graphId, profileId, maxConcurrency);
        this.enableProgressBar = enableProgressBar;
    }

    enqueueRoute = (routeId: string, startNode: number, endNode: number) => {
        if (this.processing) throw new Error("Queue is already processing. Cannot enqueue new routes.");
        return enqueueRoute(this.queueId, routeId, startNode, endNode);
    };

    getStatus = () => {
        return getQueueStatus(this.queueId);
    };

    clear = () => {
        if (this.processing) throw new Error("Cannot clear queue while processing.");
        return clearRouteQueue(this.queueId);
    };

    awaitAll = async (callback: (id: string, result: RouteResult | null, error?: Error) => void) => {
        if (this.processing) throw new Error("Queue is already processing. Cannot await new routes.");

        const initialStatus = this.getStatus();
        const totalTasks = initialStatus.queuedTasks;

        if (totalTasks === 0) {
            return Promise.resolve();
        }

        this.processing = true;

        return new Promise<void>((resolve) => {
            let completedTasks = 0;
            let emptyCount = 0;
            let bar: cliProgress.SingleBar | null = null;

            if (this.enableProgressBar) {
                bar = new cliProgress.SingleBar(
                    {
                        format: "Processing |{bar}| {percentage}% | {value}/{total} | ETA: {eta_formatted} | Speed: {speed} routes/s | Empty: {emptyCount}",
                        stopOnComplete: true,
                    },
                    cliProgress.Presets.shades_classic
                );
                bar.start(totalTasks, 0, { speed: "N/A", emptyCount: 0 });
            }

            processQueue(this.queueId, (id, result) => {
                if (result instanceof Error) {
                    callback(id, null, result);
                } else {
                    if (!result || !result.nodes || !result.nodes.length) emptyCount++;
                    callback(id, result);
                }

                completedTasks++;
                if (bar) {
                    bar.increment(1, { emptyCount });
                }

                if (completedTasks >= totalTasks) {
                    if (bar) bar.stop();
                    this.processing = false;
                    resolve();
                }
            });
        });
    };
}

export default RouteQueue;
