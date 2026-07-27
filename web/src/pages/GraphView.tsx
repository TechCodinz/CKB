import React, { useEffect, useRef, useState } from 'react';
import { useParams } from 'react-router-dom';
import {
    Box,
    Card,
    CardContent,
    Typography,
    FormControl,
    InputLabel,
    Select,
    MenuItem,
    Slider,
    Switch,
    FormControlLabel,
    Button,
    IconButton,
    Paper,
    CircularProgress,
} from '@mui/material';
import {
    ZoomIn as ZoomInIcon,
    ZoomOut as ZoomOutIcon,
    Refresh as RefreshIcon,
    Save as SaveIcon,
    Fullscreen as FullscreenIcon,
} from '@mui/icons-material';
import ForceGraph2D from 'react-force-graph-2d';
import { api } from '../services/api';

interface GraphData {
    nodes: Array<{
        id: string;
        name: string;
        type: string;
        violations?: number;
    }>;
    links: Array<{
        source: string;
        target: string;
        type: string;
    }>;
}

export default function GraphView() {
    const { id } = useParams<{ id: string }>();
    // To avoid adding notistack as dependency if not present, we will mock enqueueSnackbar for now
    // const { enqueueSnackbar } = useSnackbar();
    const enqueueSnackbar = (msg: string, opts: any) => console.log(msg, opts);

    const graphRef = useRef<any>();
    const [loading, setLoading] = useState(true);
    const [graphData, setGraphData] = useState<GraphData | null>(null);
    const [layout, setLayout] = useState('force');
    const [showLabels, setShowLabels] = useState(true);
    const [showViolations, setShowViolations] = useState(true);
    const [nodeSize, setNodeSize] = useState(1);
    const [selectedNode, setSelectedNode] = useState<any>(null);

    useEffect(() => {
        loadGraph();
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [id]);

    const loadGraph = async () => {
        try {
            const response = await api.get(`/projects/${id}/graph`);
            setGraphData(response.data);
        } catch (error) {
            enqueueSnackbar('Failed to load graph', { variant: 'error' });
            // Provide mock data for demonstration if API fails
            setGraphData({
                nodes: [
                    { id: '1', name: 'src/main.rs', type: 'file', violations: 0 },
                    { id: '2', name: 'src/engine.rs', type: 'file', violations: 2 },
                    { id: '3', name: 'core', type: 'module', violations: 0 }
                ],
                links: [
                    { source: '1', target: '2', type: 'import' },
                    { source: '2', target: '3', type: 'import' }
                ]
            });
        } finally {
            setLoading(false);
        }
    };

    const handleRefresh = async () => {
        setLoading(true);
        await loadGraph();
        setLoading(false);
    };

    const handleZoomIn = () => {
        if (graphRef.current) {
            const currentZoom = graphRef.current.zoom();
            graphRef.current.zoom(currentZoom * 1.2);
        }
    };

    const handleZoomOut = () => {
        if (graphRef.current) {
            const currentZoom = graphRef.current.zoom();
            graphRef.current.zoom(currentZoom / 1.2);
        }
    };

    const handleNodeClick = (node: any) => {
        setSelectedNode(node);
    };

    const getNodeColor = (node: any) => {
        if (node.violations > 0 && showViolations) return '#f44336';
        switch (node.type) {
            case 'file': return '#2196f3';
            case 'class': return '#4caf50';
            case 'function': return '#ff9800';
            default: return '#9e9e9e';
        }
    };

    if (loading) {
        return (
            <Box sx={{ display: 'flex', justifyContent: 'center', alignItems: 'center', height: '100%' }}>
                <CircularProgress />
            </Box>
        );
    }

    if (!graphData) {
        return (
            <Box sx={{ p: 3, textAlign: 'center' }}>
                <Typography>No graph data available</Typography>
            </Box>
        );
    }

    return (
        <Box sx={{ height: '100%', display: 'flex', flexDirection: 'column' }}>
            {/* Controls */}
            <Paper sx={{ p: 2, borderBottom: 1, borderColor: 'divider' }}>
                <Box sx={{ display: 'flex', alignItems: 'center', gap: 2, flexWrap: 'wrap' }}>
                    <FormControl size="small" sx={{ minWidth: 120 }}>
                        <InputLabel>Layout</InputLabel>
                        <Select value={layout} label="Layout" onChange={(e) => setLayout(e.target.value)}>
                            <MenuItem value="force">Force-directed</MenuItem>
                            <MenuItem value="radial">Radial</MenuItem>
                            <MenuItem value="hierarchical">Hierarchical</MenuItem>
                        </Select>
                    </FormControl>

                    <FormControlLabel
                        control={
                            <Switch
                                checked={showLabels}
                                onChange={(e) => setShowLabels(e.target.checked)}
                            />
                        }
                        label="Show Labels"
                    />

                    <FormControlLabel
                        control={
                            <Switch
                                checked={showViolations}
                                onChange={(e) => setShowViolations(e.target.checked)}
                            />
                        }
                        label="Highlight Violations"
                    />

                    <Box sx={{ width: 200 }}>
                        <Typography gutterBottom>Node Size</Typography>
                        <Slider
                            value={nodeSize}
                            onChange={(_, v) => setNodeSize(v as number)}
                            min={0.5}
                            max={3}
                            step={0.1}
                        />
                    </Box>

                    <Box sx={{ ml: 'auto', display: 'flex', gap: 1 }}>
                        <IconButton onClick={handleZoomIn}>
                            <ZoomInIcon />
                        </IconButton>
                        <IconButton onClick={handleZoomOut}>
                            <ZoomOutIcon />
                        </IconButton>
                        <IconButton onClick={handleRefresh}>
                            <RefreshIcon />
                        </IconButton>
                        <IconButton>
                            <SaveIcon />
                        </IconButton>
                        <IconButton>
                            <FullscreenIcon />
                        </IconButton>
                    </Box>
                </Box>
            </Paper>

            {/* Graph */}
            <Box sx={{ flexGrow: 1, position: 'relative' }}>
                <ForceGraph2D
                    ref={graphRef}
                    graphData={graphData}
                    nodeLabel={showLabels ? 'name' : undefined}
                    nodeColor={getNodeColor}
                    nodeVal={(node: any) => (node.violations || 1) * nodeSize}
                    linkColor={() => '#666'}
                    linkDirectionalParticles={2}
                    linkDirectionalParticleSpeed={0.005}
                    onNodeClick={handleNodeClick}
                    cooldownTime={2000}
                    warmupTicks={100}
                />

                {/* Node Info Panel */}
                {selectedNode && (
                    <Card sx={{ position: 'absolute', bottom: 16, right: 16, width: 300 }}>
                        <CardContent>
                            <Typography variant="h6">{selectedNode.name}</Typography>
                            <Typography color="textSecondary" gutterBottom>
                                Type: {selectedNode.type}
                            </Typography>
                            {selectedNode.violations > 0 && (
                                <Typography color="error">
                                    Violations: {selectedNode.violations}
                                </Typography>
                            )}
                            <Button size="small" sx={{ mt: 1 }}>
                                View Details
                            </Button>
                        </CardContent>
                    </Card>
                )}
            </Box>
        </Box>
    );
}
