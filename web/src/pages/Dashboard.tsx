import React, { useState, useEffect } from 'react';
import {
    Box, Container, Grid, Paper, Typography, Card, CardContent,
    CircularProgress, Button, Alert, LinearProgress, Chip
} from '@mui/material';
import {
    XAxis, YAxis, CartesianGrid, Tooltip,
    ResponsiveContainer, BarChart, Bar
} from 'recharts';
import {
    Assessment as AssessmentIcon,
    BugReport as BugReportIcon,
    FolderOpen as FolderIcon,
    Schedule as ScheduleIcon,
    Add as AddIcon,
    Refresh as RefreshIcon,
} from '@mui/icons-material';
import { ckbApi } from '../services/api';

interface ScanReport {
    files_processed: number;
    nodes: number;
    edges: number;
    patterns: any[];
    drift: any[];
    snapshot_id: string;
}

interface StatCardProps {
    title: string;
    value: string | number;
    color?: 'primary' | 'error' | 'warning' | 'success';
    icon: React.ReactNode;
    subtitle?: string;
}

function StatCard({ title, value, color = 'primary', icon, subtitle }: StatCardProps) {
    return (
        <Card sx={{ height: '100%', background: 'linear-gradient(135deg, #1a1a2e 0%, #16213e 100%)', border: '1px solid rgba(255,255,255,0.1)' }}>
            <CardContent>
                <Box sx={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start' }}>
                    <Box>
                        <Typography color="textSecondary" gutterBottom variant="body2" sx={{ textTransform: 'uppercase', letterSpacing: 1 }}>
                            {title}
                        </Typography>
                        <Typography variant="h3" component="div" color={`${color}.main`} sx={{ fontWeight: 700 }}>
                            {value}
                        </Typography>
                        {subtitle && (
                            <Typography variant="caption" color="textSecondary">{subtitle}</Typography>
                        )}
                    </Box>
                    <Box sx={{ color: `${color}.main`, opacity: 0.7, fontSize: 40 }}>{icon}</Box>
                </Box>
            </CardContent>
        </Card>
    );
}

const Dashboard: React.FC = () => {
    const [loading, setLoading] = useState(true);
    const [scanning, setScanning] = useState(false);
    const [report, setReport] = useState<ScanReport | null>(null);
    const [error, setError] = useState<string | null>(null);
    const [backendOnline, setBackendOnline] = useState(false);

    useEffect(() => {
        checkBackendAndLoad();
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, []);

    const checkBackendAndLoad = async () => {
        try {
            await ckbApi.health();
            setBackendOnline(true);
            await loadReport();
        } catch {
            setBackendOnline(false);
            setError('CKB server is offline. Start it with: ckb-mcp-server');
        } finally {
            setLoading(false);
        }
    };

    const loadReport = async () => {
        try {
            const resp = await ckbApi.getReport();
            setReport(resp.data);
            setError(null);
        } catch (e: any) {
            if (e?.response?.status !== 404) {
                setError('Failed to load scan report');
            }
            // 404 just means no scan yet — not an error
        }
    };

    const handleScan = async () => {
        const path = prompt('Enter the path to your project:', './');
        if (!path) return;
        setScanning(true);
        try {
            await ckbApi.scan(path);
            await loadReport();
        } catch (e: any) {
            setError(`Scan failed: ${e?.response?.data || e?.message}`);
        } finally {
            setScanning(false);
        }
    };

    const healthScore = report
        ? Math.max(0, 100 - report.drift.filter((d: any) => d.severity === 'Critical' || d.severity === 'Error').length * 5)
        : null;

    const criticalViolations = report?.drift.filter((d: any) => d.severity === 'Critical' || d.severity === 'Error').length ?? 0;
    const warningViolations = report?.drift.filter((d: any) => d.severity === 'Warning').length ?? 0;

    const violationData = report ? [
        { name: 'Critical', count: criticalViolations, fill: '#f44336' },
        { name: 'Errors', count: report.drift.filter((d: any) => d.severity === 'Error').length, fill: '#ff5722' },
        { name: 'Warnings', count: warningViolations, fill: '#ff9800' },
        { name: 'Info', count: report.drift.filter((d: any) => d.severity === 'Info').length, fill: '#2196f3' },
    ] : [];

    if (loading) {
        return (
            <Box sx={{ display: 'flex', flexDirection: 'column', justifyContent: 'center', alignItems: 'center', height: '100vh', gap: 2 }}>
                <CircularProgress size={60} />
                <Typography color="textSecondary">Connecting to CKB engine...</Typography>
            </Box>
        );
    }

    return (
        <Box sx={{ background: 'linear-gradient(180deg, #0a0a1a 0%, #0d1117 100%)', minHeight: '100vh' }}>
            <Container maxWidth="xl" sx={{ pt: 4, pb: 4 }}>
                {/* Header */}
                <Box sx={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', mb: 4 }}>
                    <Box>
                        <Typography variant="h4" sx={{ fontWeight: 800, background: 'linear-gradient(90deg, #90caf9 0%, #ce93d8 100%)', WebkitBackgroundClip: 'text', WebkitTextFillColor: 'transparent' }}>
                            CKB Dashboard
                        </Typography>
                        <Typography variant="body2" color="textSecondary">
                            Code Knowledge Base — Architectural Intelligence
                        </Typography>
                    </Box>
                    <Box sx={{ display: 'flex', gap: 2, alignItems: 'center' }}>
                        <Chip
                            label={backendOnline ? 'Engine Online' : 'Engine Offline'}
                            color={backendOnline ? 'success' : 'error'}
                            size="small"
                            variant="outlined"
                        />
                        <Button variant="outlined" startIcon={<RefreshIcon />} onClick={loadReport} disabled={!backendOnline}>
                            Refresh
                        </Button>
                        <Button variant="contained" startIcon={scanning ? <CircularProgress size={16} /> : <AddIcon />}
                            onClick={handleScan} disabled={scanning || !backendOnline}
                            sx={{ background: 'linear-gradient(90deg, #90caf9 0%, #ce93d8 100%)', color: '#000', fontWeight: 700 }}>
                            {scanning ? 'Scanning...' : 'Scan Project'}
                        </Button>
                    </Box>
                </Box>

                {scanning && <LinearProgress sx={{ mb: 2, borderRadius: 1 }} />}

                {error && (
                    <Alert severity="warning" sx={{ mb: 3 }} onClose={() => setError(null)}>
                        {error}
                    </Alert>
                )}

                {!report && !scanning && (
                    <Paper sx={{ p: 6, textAlign: 'center', background: 'rgba(255,255,255,0.03)', border: '1px dashed rgba(255,255,255,0.2)', borderRadius: 3 }}>
                        <AssessmentIcon sx={{ fontSize: 64, color: 'primary.main', opacity: 0.5, mb: 2 }} />
                        <Typography variant="h5" gutterBottom>No scan yet</Typography>
                        <Typography color="textSecondary" sx={{ mb: 3 }}>
                            Run your first scan to see architectural insights, violations, and dependency graphs.
                        </Typography>
                        <Button variant="contained" size="large" onClick={handleScan} disabled={!backendOnline}
                            sx={{ background: 'linear-gradient(90deg, #90caf9 0%, #ce93d8 100%)', color: '#000', fontWeight: 700, px: 4 }}>
                            Scan a Project
                        </Button>
                        {!backendOnline && (
                            <Typography variant="caption" display="block" color="error" sx={{ mt: 2 }}>
                                Start the CKB server first: <code>ckb serve</code>
                            </Typography>
                        )}
                    </Paper>
                )}

                {report && (
                    <Grid container spacing={3}>
                        {/* Stat Cards */}
                        <Grid item xs={12} md={3}>
                            <StatCard title="Health Score" value={healthScore !== null ? `${healthScore}/100` : '—'}
                                color={healthScore !== null && healthScore >= 80 ? 'success' : healthScore !== null && healthScore >= 60 ? 'warning' : 'error'}
                                icon={<AssessmentIcon fontSize="inherit" />}
                                subtitle="Based on violations found" />
                        </Grid>
                        <Grid item xs={12} md={3}>
                            <StatCard title="Violations" value={report.drift.length}
                                color={report.drift.length === 0 ? 'success' : criticalViolations > 0 ? 'error' : 'warning'}
                                icon={<BugReportIcon fontSize="inherit" />}
                                subtitle={`${criticalViolations} critical, ${warningViolations} warnings`} />
                        </Grid>
                        <Grid item xs={12} md={3}>
                            <StatCard title="Files Analyzed" value={report.files_processed.toLocaleString()}
                                color="primary"
                                icon={<FolderIcon fontSize="inherit" />}
                                subtitle={`${report.nodes} nodes, ${report.edges} edges`} />
                        </Grid>
                        <Grid item xs={12} md={3}>
                            <StatCard title="Patterns Found" value={report.patterns.length}
                                color="primary"
                                icon={<ScheduleIcon fontSize="inherit" />}
                                subtitle="Architectural patterns detected" />
                        </Grid>

                        {/* Violation Breakdown Chart */}
                        {report.drift.length > 0 && (
                            <Grid item xs={12} md={6}>
                                <Paper sx={{ p: 3, background: 'rgba(255,255,255,0.03)', border: '1px solid rgba(255,255,255,0.1)', borderRadius: 3 }}>
                                    <Typography variant="h6" gutterBottom sx={{ fontWeight: 600 }}>Violations by Severity</Typography>
                                    <ResponsiveContainer width="100%" height={220}>
                                        <BarChart data={violationData}>
                                            <CartesianGrid strokeDasharray="3 3" stroke="rgba(255,255,255,0.1)" />
                                            <XAxis dataKey="name" tick={{ fill: '#aaa' }} />
                                            <YAxis tick={{ fill: '#aaa' }} />
                                            <Tooltip contentStyle={{ background: '#1a1a2e', border: '1px solid rgba(255,255,255,0.2)' }} />
                                            <Bar dataKey="count" fill="#90caf9" radius={[4, 4, 0, 0]} />
                                        </BarChart>
                                    </ResponsiveContainer>
                                </Paper>
                            </Grid>
                        )}

                        {/* Top Violations List */}
                        {report.drift.length > 0 && (
                            <Grid item xs={12} md={6}>
                                <Paper sx={{ p: 3, background: 'rgba(255,255,255,0.03)', border: '1px solid rgba(255,255,255,0.1)', borderRadius: 3, height: '100%' }}>
                                    <Typography variant="h6" gutterBottom sx={{ fontWeight: 600 }}>Top Violations</Typography>
                                    <Box sx={{ overflowY: 'auto', maxHeight: 220 }}>
                                        {report.drift.slice(0, 8).map((v: any, i: number) => (
                                            <Box key={i} sx={{ display: 'flex', alignItems: 'center', gap: 1.5, py: 1, borderBottom: '1px solid rgba(255,255,255,0.06)' }}>
                                                <Chip
                                                    label={v.severity}
                                                    size="small"
                                                    color={v.severity === 'Critical' ? 'error' : v.severity === 'Warning' ? 'warning' : 'default'}
                                                    sx={{ minWidth: 70 }}
                                                />
                                                <Typography variant="body2" sx={{ flex: 1 }} noWrap>{v.message}</Typography>
                                            </Box>
                                        ))}
                                    </Box>
                                </Paper>
                            </Grid>
                        )}

                        {/* Snapshot ID */}
                        <Grid item xs={12}>
                            <Paper sx={{ p: 2, background: 'rgba(255,255,255,0.02)', border: '1px solid rgba(255,255,255,0.06)', borderRadius: 2 }}>
                                <Typography variant="caption" color="textSecondary">
                                    Snapshot ID: <code>{report.snapshot_id}</code> — Scan complete. Use <strong>ckb export</strong> to generate Mermaid/DOT diagrams, or click a project to explore the graph.
                                </Typography>
                            </Paper>
                        </Grid>
                    </Grid>
                )}
            </Container>
        </Box>
    );
};

export default Dashboard;
