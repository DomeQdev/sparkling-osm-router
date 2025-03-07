import { existsSync, statSync, writeFileSync } from "fs";
import { Location, OSMGraphOptions } from ".";

export default async ({ path, ttl, bounds, overpassQuery }: OSMGraphOptions) => {
    if (existsSync(path) && statSync(path).mtime.getTime() > Date.now() - ttl * 60 * 60 * 1000) return;

    const query = getOverpassQuery(bounds, overpassQuery);

    let response: string | undefined;

    for (let i = 0; i < 5; i++) {
        try {
            response = await fetch("https://overpass.private.coffee/api/interpreter", {
                method: "POST",
                body: query,
            }).then((res) => res.text());

            if (response?.includes("Error")) {
                const errorMessage = response.match(/<strong[^>]*>Error<\/strong>:([^<]*)/)?.[1]?.trim();
                throw new Error(`Overpass API Error: ${errorMessage}`);
            }

            break;
        } catch (e) {
            console.error(`Try ${i + 1}/5 returned error:`, e);
        }
    }

    if (!response) throw new Error("Failed to fetch data from OSM");

    writeFileSync(path, response);
};

const getOverpassQuery = (bounds: Location[], query: string) => {
    return `
        [out:xml][timeout:100000];
        ${query}
        (poly: "${bounds.map(([lon, lat]) => `${lat.toFixed(5)} ${lon.toFixed(5)}`).join(" ")}");

        >->.n;
        <->.r;
        (._;.n;.r;);
        out;
    `;
};
