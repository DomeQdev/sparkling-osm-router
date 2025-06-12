import { Location } from "../typings";

const perpendicularDistance = (
    [lon, lat]: Location,
    [lon1, lat1]: Location,
    [lon2, lat2]: Location
): number => {
    const lineLengthSquared = Math.pow(lon2 - lon1, 2) + Math.pow(lat2 - lat1, 2);

    if (lineLengthSquared === 0.0) {
        return Math.sqrt(Math.pow(lon - lon1, 2) + Math.pow(lat - lat1, 2));
    }

    const clampedT = Math.max(
        0,
        Math.min(1, ((lon - lon1) * (lon2 - lon1) + (lat - lat1) * (lat2 - lat1)) / lineLengthSquared)
    );

    const dx = lon - (lon1 + clampedT * (lon2 - lon1));
    const dy = lat - (lat1 + clampedT * (lat2 - lat1));

    return Math.sqrt(dx * dx + dy * dy);
};

const findFurthestPoint = (points: Location[]): { index: number; distance: number } => {
    if (points.length <= 2) {
        return { index: 0, distance: 0.0 };
    }

    let maxDistance = 0.0;
    let maxIndex = 0;

    for (let i = 1; i < points.length - 1; i++) {
        const distance = perpendicularDistance(points[i], points[0], points[points.length - 1]);
        if (distance > maxDistance) {
            maxDistance = distance;
            maxIndex = i;
        }
    }

    return { index: maxIndex, distance: maxDistance };
};

const rdpSimplify = (points: Location[], epsilon: number): Location[] => {
    if (points.length <= 2) return points;

    let result: Location[] = [];
    const { index: furthestIndex, distance: furthestDistance } = findFurthestPoint(points);

    if (furthestDistance > epsilon) {
        const simplifiedFirst = rdpSimplify(points.slice(0, furthestIndex + 1), epsilon);
        const simplifiedSecond = rdpSimplify(points.slice(furthestIndex), epsilon);

        result = simplifiedFirst.slice(0, simplifiedFirst.length - 1).concat(simplifiedSecond);
    } else {
        result.push(points[0]);
        result.push(points[points.length - 1]);
    }

    return result;
};

export default (shapePoints: Location[], epsilon: number = 1e-5): Location[] => {
    if (!shapePoints?.length || epsilon <= 0) return shapePoints;

    return rdpSimplify(shapePoints, epsilon);
};
