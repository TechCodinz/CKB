import React, { useState, useEffect } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import {
    Box, Container, Typography, Tabs, Tab, Paper, List, ListItem,
    ListItemText, ListItemIcon, Divider, Chip, Button, TextField,
    CircularProgress, Alert, Slider, FormControlLabel, Switch
} from '@mui/material';
import {
    InsertDriveFile as FileIcon,
    Warning as WarningIcon, Error as ErrorIcon,
    CheckCircle as CheckIcon, Search as SearchIcon
} from '@mui/icons-material';
import Editor from '@monaco-editor/react';
import ForceGraph2D from 'react-force-graph-2d';
import { ckbApi } from '../services/api';

interface TabPanelProps {
    children?: React.ReactNode;
    index: number;
    value: number;
}

function TabPanel({ children, value, index }: TabPanelProps) {
    return (
        <div role="tabpanel" hidden={value !== index} style={{ height: 'calc(100vh - 220px)', overflow: 'auto' }}>
            {value === index && <Box sx={{ p: 2, height: '100%' }}>{children}</Box>}
        </div>
    );
}

function severityColor(s: string): 'error' | 'warning' | 'info' | 'success' | 'default' {
    if (s === 'Critical' || s === 'Error') return 'error';
    if (s === 'Warning') return 'warning';
    if (s === 'Info') return 'info';
    return 'default';
}

const ProjectView: React.FC = () => {
    const { id } = useParams<{ id: string }>();
    const navigate = useNavigate();
    const [tabValue, setTabValue] = useState(0);
    const [selectedFile, setSelectedFile] = useState<string | null>(null);
    const [report, setReport] = useState<any>(null);
    const [graphData, setGraphData] = useState<any>(null);
    const [graphLoading, setGraphLoading] = useState(false);
    const [impactFile, setImpactFile] = useState('');
    const [impactLine, setImpactLine] = useState(1);
    const [impactResult, setImpactResult] = useState<any>(null);
    const [impactLoading, setImpactLoading] = useState(false);
    const [impactError, setImpactError] = useState('');
    const [showLabels, setShowLabels] = useState(true);
    const [nodeSize, setNodeSize] = useState(1);

    useEffect(() => {
        loadReport();
    }, []);

    const loadReport = async () => {
        try {
            const resp = await ckbApi.getReport();
            setReport(resp.data);
        } catch {
            // no report yet
        }
    };

    const loadGraph = async () => {
        setGraphLoading(true);
        try {
            // Build graph from report data
            const resp = await ckbApi.getReport();
            const r = resp.data;
            // Map drift violations to graph links
            const nodes = (r.drift || []).reduce((acc: any[], v: any) => {
                if (v.from && !acc.find((n: any) => n.id === v.from['0'])) {
                    acc.push({ id: v.from['0'] || String(v.from), name: v.from['0'] || String(v.from), type: 'file', violations: 1 });
                }
                return acc;
            }, [{ id: 'root', name: 'Project Root', type: 'module', violations: 0 }]);

            // Use project graph endpoint if project ID is provided
            if (id && id !== 'current') {
                try {
                    const gResp = await ckbApi.getProjectGraph(id);
                    setGraphData(gResp.data);
                    return;
                } catch { /* fallback */ }
            }

            setGraphData({
                nodes: nodes.slice(0, 50),
                links: (r.drift || []).slice(0, 30).map((v: any) => ({
                    source: v.from?.['0'] || 'root',
                    target: v.to?.['0'] || 'root',
                    type: v.kind,
                }))
            });
        } catch {
            setGraphData({ nodes: [], links: [] });
        } finally {
            setGraphLoading(false);
        }
    };

    const handleTabChange = (_: React.SyntheticEvent, newValue: number) => {
        setTabValue(newValue);
        if (newValue === 2 && !graphData) loadGraph();
    };

    const handleImpactAnalysis = async () => {
        if (!impactFile) return;
        setImpactLoading(true);
        setImpactError('');
        setImpactResult(null);
        try {
            const resp = await ckbApi.analyzeImpact('./', impactFile, impactLine, 'modify');
            setImpactResult(resp.data);
        } catch (e: any) {
            setImpactError(e?.response?.data || e?.message || 'Analysis failed. Make sure the project is scanned first.');
        } finally {
            setImpactLoading(false);
        }
    };

    const getNodeColor = (node: any) => {
        if (node.violations > 0) return '#f44336';
        switch (node.type) {
            case 'file': return '#90caf9';
            case 'class': return '#4caf50';
            case 'function': return '#ff9800';
            case 'module': return '#ce93d8';
            default: return '#666';
        }
    };

    const violations = report?.drift || [];
    const criticalViolations = violations.filter((v: any) => v.severity === 'Critical' || v.severity === 'Error');
    const otherViolations = violations.filter((v: any) => v.severity !== 'Critical' && v.severity !== 'Error');

    return (
        <Box sx={{ background: '#0d1117', minHeight: '100vh' }}>
            <Container maxWidth="xl" sx={{ pt: 3 }}>
                <Box sx={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', mb: 2 }}>
                    <Box>
                        <Typography variant="h5" fontWeight={700}>
                            Project Analysis
                        </Typography>
                        <Typography variant="body2" color="textSecondary">
                            {id || 'Current Scan'}
                        </Typography>
                    </Box>
                    <Button variant="outlined" onClick={() => navigate('/')}>← Dashboard</Button>
                </Box>

                <Paper sx={{ width: '100%', background: 'rgba(255,255,255,0.03)', border: '1px solid rgba(255,255,255,0.1)' }}>
                    <Tabs value={tabValue} onChange={handleTabChange}
                        indicatorColor="primary" textColor="primary"
                        sx={{ borderBottom: '1px solid rgba(255,255,255,0.1)' }}>
                        <Tab label="Code View" />
                        <Tab label={`Violations (${violations.length})`} />
                        <Tab label="Dependency Graph" />
                        <Tab label="Impact Analysis" />
                    </Tabs>

                    {/* FILES TAB */}
                    <TabPanel value={tabValue} index={0}>
                        <Box sx={{ display: 'flex', height: '100%', gap: 2 }}>
                            <Box sx={{ width: 280, borderRight: '1px solid rgba(255,255,255,0.1)', overflowY: 'auto' }}>
                                <Typography variant="caption" color="textSecondary" sx={{ px: 2, py: 1, display: 'block' }}>FILES WITH VIOLATIONS</Typography>
                                <List dense>
                                    {criticalViolations.slice(0, 20).map((v: any, i: number) => {
                                        const file = v.from?.['0'] || String(v.from);
                                        return (
                                            <ListItem key={i} button onClick={() => setSelectedFile(file)}
                                                selected={selectedFile === file}
                                                sx={{ '&.Mui-selected': { background: 'rgba(144,202,249,0.1)' } }}>
                                                <ListItemIcon sx={{ minWidth: 30 }}><FileIcon fontSize="small" color="error" /></ListItemIcon>
                                                <ListItemText
                                                    primary={file.split('/').pop() || file}
                                                    secondary={file}
                                                    primaryTypographyProps={{ variant: 'body2', noWrap: true }}
                                                    secondaryTypographyProps={{ variant: 'caption', noWrap: true }}
                                                />
                                            </ListItem>
                                        );
                                    })}
                                    {violations.length === 0 && (
                                        <ListItem>
                                            <ListItemIcon><CheckIcon color="success" /></ListItemIcon>
                                            <ListItemText primary="No violations found" secondary="Architecture looks clean!" />
                                        </ListItem>
                                    )}
                                </List>
                            </Box>
                            <Box sx={{ flex: 1 }}>
                                {selectedFile ? (
                                    <Editor
                                        height="100%"
                                        defaultLanguage="typescript"
                                        theme="vs-dark"
                                        value={`// ${selectedFile}\n// Select this file in your editor to view full content.\n// CKB has identified violations in this file.\n\n// Violations:\n${violations.filter((v: any) => (v.from?.['0'] || '') === selectedFile).map((v: any) => `// ⚠ ${v.message}`).join('\n')}`}
                                        options={{ readOnly: true, minimap: { enabled: false }, fontSize: 13 }}
                                    />
                                ) : (
                                    <Box sx={{ height: '100%', display: 'flex', alignItems: 'center', justifyContent: 'center', flexDirection: 'column', gap: 1 }}>
                                        <FileIcon sx={{ fontSize: 48, color: 'text.disabled' }} />
                                        <Typography color="textSecondary">Select a file from the sidebar</Typography>
                                    </Box>
                                )}
                            </Box>
                        </Box>
                    </TabPanel>

                    {/* VIOLATIONS TAB */}
                    <TabPanel value={tabValue} index={1}>
                        {violations.length === 0 ? (
                            <Box sx={{ textAlign: 'center', py: 6 }}>
                                <CheckIcon sx={{ fontSize: 64, color: 'success.main', mb: 2 }} />
                                <Typography variant="h6">No violations found!</Typography>
                                <Typography color="textSecondary">Your architecture is clean. Keep it that way.</Typography>
                            </Box>
                        ) : (
                            <List>
                                {[...criticalViolations, ...otherViolations].map((v: any, i: number) => (
                                    <React.Fragment key={i}>
                                        <ListItem alignItems="flex-start" sx={{ py: 1.5 }}>
                                            <ListItemIcon sx={{ mt: 0.5 }}>
                                                {v.severity === 'Critical' || v.severity === 'Error'
                                                    ? <ErrorIcon color="error" />
                                                    : <WarningIcon color="warning" />}
                                            </ListItemIcon>
                                            <ListItemText
                                                primary={
                                                    <Box sx={{ display: 'flex', gap: 1, alignItems: 'center', mb: 0.5 }}>
                                                        <Chip label={v.severity} size="small" color={severityColor(v.severity)} />
                                                        <Chip label={v.kind} size="small" variant="outlined" />
                                                        <Typography variant="body2" fontWeight={600}>{v.boundary}</Typography>
                                                    </Box>
                                                }
                                                secondary={
                                                    <Box>
                                                        <Typography variant="body2" color="text.primary">{v.message}</Typography>
                                                        {v.suggested_fix && (
                                                            <Typography variant="caption" color="success.main">
                                                                💡 {v.suggested_fix}
                                                            </Typography>
                                                        )}
                                                    </Box>
                                                }
                                            />
                                        </ListItem>
                                        {i < violations.length - 1 && <Divider component="li" />}
                                    </React.Fragment>
                                ))}
                            </List>
                        )}
                    </TabPanel>

                    {/* GRAPH TAB */}
                    <TabPanel value={tabValue} index={2}>
                        <Box sx={{ height: '100%', display: 'flex', flexDirection: 'column', gap: 1 }}>
                            <Box sx={{ display: 'flex', gap: 2, alignItems: 'center', flexWrap: 'wrap' }}>
                                <FormControlLabel
                                    control={<Switch checked={showLabels} onChange={e => setShowLabels(e.target.checked)} />}
                                    label="Labels"
                                />
                                <Box sx={{ width: 150 }}>
                                    <Typography variant="caption">Node Size</Typography>
                                    <Slider value={nodeSize} onChange={(_, v) => setNodeSize(v as number)} min={0.5} max={3} step={0.1} />
                                </Box>
                                <Button size="small" onClick={loadGraph} disabled={graphLoading} startIcon={graphLoading ? <CircularProgress size={14} /> : <SearchIcon />}>
                                    Reload Graph
                                </Button>
                            </Box>
                            <Box sx={{ flex: 1, border: '1px solid rgba(255,255,255,0.1)', borderRadius: 2, overflow: 'hidden', background: '#0a0a1a' }}>
                                {graphLoading ? (
                                    <Box sx={{ display: 'flex', justifyContent: 'center', alignItems: 'center', height: '100%' }}>
                                        <CircularProgress />
                                    </Box>
                                ) : graphData && graphData.nodes.length > 0 ? (
                                    <ForceGraph2D
                                        graphData={graphData}
                                        nodeLabel={showLabels ? 'name' : undefined}
                                        nodeColor={getNodeColor}
                                        nodeVal={(node: any) => (node.violations || 1) * nodeSize}
                                        linkColor={() => 'rgba(255,255,255,0.2)'}
                                        linkDirectionalParticles={2}
                                        linkDirectionalParticleSpeed={0.005}
                                        backgroundColor="#0a0a1a"
                                        cooldownTime={2000}
                                        warmupTicks={100}
                                        width={window.innerWidth - 200}
                                        height={window.innerHeight - 350}
                                    />
                                ) : (
                                    <Box sx={{ display: 'flex', justifyContent: 'center', alignItems: 'center', height: '100%', flexDirection: 'column', gap: 1 }}>
                                        <Typography color="textSecondary">No graph data. Scan a project first.</Typography>
                                        <Button variant="outlined" size="small" onClick={loadGraph}>Load Graph</Button>
                                    </Box>
                                )}
                            </Box>
                        </Box>
                    </TabPanel>

                    {/* IMPACT ANALYSIS TAB */}
                    <TabPanel value={tabValue} index={3}>
                        <Box sx={{ maxWidth: 600 }}>
                            <Typography variant="h6" fontWeight={600} mb={1}>Impact Analysis</Typography>
                            <Typography variant="body2" color="textSecondary" mb={3}>
                                Enter a file path and line number to see exactly what would break if you changed it.
                            </Typography>

                            <Box sx={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
                                <TextField
                                    id="impact-file"
                                    label="File path (relative to project root)"
                                    placeholder="src/auth/login.ts"
                                    value={impactFile}
                                    onChange={e => setImpactFile(e.target.value)}
                                    fullWidth
                                    size="small"
                                />
                                <TextField
                                    id="impact-line"
                                    label="Line number"
                                    type="number"
                                    value={impactLine}
                                    onChange={e => setImpactLine(Number(e.target.value))}
                                    size="small"
                                    sx={{ maxWidth: 180 }}
                                />
                                <Button
                                    variant="contained"
                                    onClick={handleImpactAnalysis}
                                    disabled={impactLoading || !impactFile}
                                    startIcon={impactLoading ? <CircularProgress size={16} /> : <SearchIcon />}
                                    sx={{ width: 'fit-content', background: 'linear-gradient(90deg, #90caf9 0%, #ce93d8 100%)', color: '#000', fontWeight: 700 }}
                                >
                                    {impactLoading ? 'Analyzing...' : 'Analyze Impact'}
                                </Button>

                                {impactError && <Alert severity="error">{impactError}</Alert>}

                                {impactResult && (
                                    <Box>
                                        <Alert severity={impactResult.risk_score > 0.7 ? 'error' : impactResult.risk_score > 0.4 ? 'warning' : 'success'}
                                            sx={{ mb: 2 }}>
                                            Risk Score: <strong>{(impactResult.risk_score * 100).toFixed(0)}%</strong> —
                                            {impactResult.risk_score > 0.7 ? ' High risk change. Proceed with caution.'
                                                : impactResult.risk_score > 0.4 ? ' Medium risk. Test downstream dependencies.'
                                                    : ' Low risk change.'}
                                        </Alert>

                                        {impactResult.directly_affected?.length > 0 && (
                                            <Box>
                                                <Typography variant="subtitle2" fontWeight={600} mb={1}>
                                                    Directly Affected ({impactResult.directly_affected.length})
                                                </Typography>
                                                <List dense>
                                                    {impactResult.directly_affected.map((file: any, i: number) => (
                                                        <ListItem key={i} sx={{ py: 0 }}>
                                                            <ListItemIcon sx={{ minWidth: 28 }}><FileIcon fontSize="small" color="warning" /></ListItemIcon>
                                                            <ListItemText
                                                                primary={typeof file === 'string' ? file : file['0'] || JSON.stringify(file)}
                                                                primaryTypographyProps={{ variant: 'body2' }}
                                                            />
                                                        </ListItem>
                                                    ))}
                                                </List>
                                            </Box>
                                        )}

                                        {impactResult.transitively_affected?.length > 0 && (
                                            <Box sx={{ mt: 2 }}>
                                                <Typography variant="subtitle2" fontWeight={600} mb={1}>
                                                    Transitively Affected ({impactResult.transitively_affected.length})
                                                </Typography>
                                                <List dense>
                                                    {impactResult.transitively_affected.slice(0, 10).map((file: any, i: number) => (
                                                        <ListItem key={i} sx={{ py: 0 }}>
                                                            <ListItemIcon sx={{ minWidth: 28 }}><FileIcon fontSize="small" color="info" /></ListItemIcon>
                                                            <ListItemText
                                                                primary={typeof file === 'string' ? file : file['0'] || JSON.stringify(file)}
                                                                primaryTypographyProps={{ variant: 'body2' }}
                                                            />
                                                        </ListItem>
                                                    ))}
                                                    {impactResult.transitively_affected.length > 10 && (
                                                        <ListItem>
                                                            <ListItemText secondary={`... and ${impactResult.transitively_affected.length - 10} more`} />
                                                        </ListItem>
                                                    )}
                                                </List>
                                            </Box>
                                        )}
                                    </Box>
                                )}
                            </Box>
                        </Box>
                    </TabPanel>
                </Paper>
            </Container>
        </Box>
    );
};

export default ProjectView;
