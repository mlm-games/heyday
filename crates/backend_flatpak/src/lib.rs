use domain::*;

#[allow(dead_code)]
pub struct FlatpakBackend {
    user: bool,
}

impl FlatpakBackend {
    pub fn new(user: bool) -> Self {
        Self { user }
    }
}

impl PackageBackend for FlatpakBackend {
    fn name(&self) -> &'static str {
        "flatpak"
    }

    fn refresh(&self, sink: &ProgressSink, cancel: &CancelToken) -> Result<()> {
        let _ = (sink, cancel);
        log::info!("flatpak refresh");
        Ok(())
    }

    fn search(
        &self,
        _q: &str,
        _sink: &ProgressSink,
        _cancel: &CancelToken,
    ) -> Result<Vec<PackageSummary>> {
        Ok(vec![])
    }

    fn details(
        &self,
        _id: &PackageId,
        _sink: &ProgressSink,
        _cancel: &CancelToken,
    ) -> Result<PackageDetails> {
        Err(Error::Flatpak("not implemented".into()))
    }

    fn installed(&self, _cancel: &CancelToken) -> Result<Vec<PackageSummary>> {
        Ok(vec![])
    }

    fn updates(&self, _cancel: &CancelToken) -> Result<Vec<PackageSummary>> {
        Ok(vec![])
    }

    fn operation(
        &self,
        _op: &Operation,
        _sink: &ProgressSink,
        _cancel: &CancelToken,
        _progress: Box<dyn FnMut(f32) + Send + 'static>,
    ) -> Result<()> {
        Err(Error::Flatpak("not implemented".into()))
    }

    fn install(&self, _id: &PackageId, _sink: &ProgressSink, _cancel: &CancelToken) -> Result<()> {
        Err(Error::Flatpak("not implemented".into()))
    }

    fn remove(&self, _id: &PackageId, _sink: &ProgressSink, _cancel: &CancelToken) -> Result<()> {
        Err(Error::Flatpak("not implemented".into()))
    }

    fn upgrade(&self, _id: &PackageId, _sink: &ProgressSink, _cancel: &CancelToken) -> Result<()> {
        Err(Error::Flatpak("not implemented".into()))
    }

    fn upgrade_all(&self, _sink: &ProgressSink, _cancel: &CancelToken) -> Result<()> {
        Err(Error::Flatpak("not implemented".into()))
    }
}
