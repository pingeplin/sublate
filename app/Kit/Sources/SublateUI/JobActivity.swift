/// What the main window is doing; an update must not quit the app in the middle of a job.
@MainActor
protocol JobActivity: AnyObject {
    var isBusy: Bool { get }
}
